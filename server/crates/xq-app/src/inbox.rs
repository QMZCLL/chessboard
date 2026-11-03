//! 复盘收件箱：一次导入多盘自己下的棋，在引擎空闲时逐盘分析，分析完成后一次为它们出题。
//!
//! - 预览只读不写：拆分多盘、解析、查重（与已有的棋局、同一批中前面的棋局比较指纹），按设置中的“我的名字”
//!   与棋谱中的红方、黑方比对，判断哪一方是自己；比对不上时由用户在预览中选择。
//! - 导入时再查一次重复。棋局照常保存为学习会话，收件箱只另外记下成员、对局信息与出题结果（`inbox` 表），
//!   不改棋局的保存格式；棋局与收件箱记录在同一个写事务中写入。
//! - 每一着都排队详细讲解（不受“走子后自动讲解”影响），用空闲优先级：有其他任务时让出引擎，
//!   不挡住正在查看的棋局；启动时为还没有分析完的着法补排。
//! - 出题不自动进行：生成的题目当天到期，一次导入很多盘时会让“今日学习”一下子多出很多题。
//!   “出题”只为已经分析完、还没有出过题的棋局出题（用现有的出题规则），并汇总新增了几道。

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;
use xq_core::Color;
use xq_core::record;
use xq_core::record::ParsedRecord;
use xq_core::record::RecordMeta;
use xq_core::study::Study;
use xq_core::study::StudyLimits;
use xq_core::study::StudySource;

use crate::archive::short;
use crate::archive::too_large;
use crate::engine::Priority;
use crate::events;
use crate::records::Budget;
use crate::service::ApiResult;
use crate::service::AppService;
use crate::service::Inner;
use crate::service::StudyStart;
use crate::service::now_ms;
use crate::transfer::MAX_IMPORT_BYTES;

/// 一次最多导入的盘数。
pub const MAX_GAMES: usize = 200;

/// 预览的来源：粘贴的文字（可以带文件名），或桌面应用中选择的文件。每份可以含多盘。
#[derive(Clone, Debug, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum InboxSource {
    Text { name: Option<String>, text: String },
    File { path: String },
}

#[derive(Clone, Debug, Deserialize, TS)]
#[ts(export)]
pub struct InboxPreviewRequest {
    pub sources: Vec<InboxSource>,
}

/// 预览中的一盘。
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct InboxCandidate {
    /// 在这次预览中的序号（从 0 开始）。
    pub index: u32,
    /// 来自哪个文件（粘贴的文字为空）。
    pub source: Option<String>,
    /// 导入后的名称：双方棋手，或赛事名，或文件名。
    pub name: String,
    pub red: Option<String>,
    pub black: Option<String>,
    pub date: Option<String>,
    pub result: Option<String>,
    /// 能导入的着法数。
    pub plies: u32,
    /// 解析停在哪一步（此前的着法照样导入）。
    pub stopped: Option<String>,
    /// 不能导入的原因。
    pub error: Option<String>,
    /// 与哪一盘重复（重复的默认不导入）。
    pub duplicate: Option<String>,
    /// 按“我的名字”判断的我方；比对不上时为空，需要用户选择。
    pub side: Option<Color>,
    /// 判断我方的依据，或需要选择的原因。
    pub side_reason: String,
    /// 这一盘的棋谱文字：导入时原样交回。
    pub text: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct InboxPreview {
    pub games: Vec<InboxCandidate>,
    /// 读取文件失败、超过盘数上限等说明。
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, TS)]
#[ts(export)]
pub struct InboxImportGame {
    pub text: String,
    pub name: Option<String>,
    pub user_side: Color,
}

#[derive(Clone, Debug, Deserialize, TS)]
#[ts(export)]
pub struct InboxImportRequest {
    pub games: Vec<InboxImportGame>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct InboxImportResult {
    pub added: u32,
    /// 没有导入的棋局与原因。
    pub skipped: Vec<String>,
    pub text: String,
}

/// 为一盘出题的结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InboxExercises {
    pub at: i64,
    /// 新增的题数。
    pub added: u32,
    pub text: String,
}

/// 收件箱中的一盘（保存在 `inbox` 表中）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxEntry {
    pub study_id: String,
    /// 加入的时间：同一批按导入顺序递增，没有日期的棋局按它排列。
    pub added_at: i64,
    #[serde(default)]
    pub red: Option<String>,
    #[serde(default)]
    pub black: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub exercises: Option<InboxExercises>,
    /// 分析完成时整局报告选出的关键局面数：第一次列出分析完的这盘时记下，之后不再每次重新计算；
    /// 这盘需要重新分析（例如讲解规则更新）时清除。
    #[serde(default)]
    pub key_positions: Option<u32>,
}

/// 列表中的一盘：分析进度与出题结果。
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct InboxItem {
    pub study_id: String,
    pub name: String,
    pub user_side: Color,
    pub red: Option<String>,
    pub black: Option<String>,
    pub date: Option<String>,
    pub result: Option<String>,
    pub added_at: i64,
    pub moves: u32,
    /// 已经得到分析结果的着法（含分析不完整的）。
    pub analysed: u32,
    /// 分析不完整或失败的着法。
    pub incomplete: u32,
    /// 每一着都已经分析过。
    pub done: bool,
    /// 分析完成后，整局报告选出的关键局面数。
    pub key_positions: Option<u32>,
    pub exercises: Option<InboxExercises>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct InboxList {
    pub items: Vec<InboxItem>,
    /// 一句话说明：共几盘、几盘分析完、几盘可以出题。
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[ts(export)]
pub struct InboxGenerateResult {
    /// 这次出题的盘数。
    pub games: u32,
    pub threat: u32,
    pub choice: u32,
    pub better: u32,
    pub existing: u32,
    /// 还没有分析完、这次没有出题的盘数。
    pub waiting: u32,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InboxRef {
    pub study_id: String,
}

/// 比较名字用的写法：去掉空白，全角字母数字换成半角，不分大小写。
fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// 按“我的名字”判断哪一方是自己，返回我方与依据；比对不上时我方为空，依据写明需要选择的原因。
pub fn match_side(meta: &RecordMeta, names: &[String]) -> (Option<Color>, String) {
    let keys: HashSet<String> = names.iter().map(|n| name_key(n)).filter(|k| !k.is_empty()).collect();
    let hit = |p: &Option<String>| p.as_deref().is_some_and(|n| keys.contains(&name_key(n)));
    let (red, black) = (hit(&meta.red), hit(&meta.black));
    let quote = |p: &Option<String>| p.as_deref().map(|n| format!("“{}”", short(n))).unwrap_or_default();
    match (red, black) {
        (true, false) => (Some(Color::Red), format!("红方{}与设置中的名字相同", quote(&meta.red))),
        (false, true) => (Some(Color::Black), format!("黑方{}与设置中的名字相同", quote(&meta.black))),
        (true, true) => (None, "红黑双方的名字都与设置中的名字相同，请选择".into()),
        (false, false) if meta.red.is_none() && meta.black.is_none() => (None, "棋谱中没有写明双方棋手，请选择".into()),
        (false, false) if keys.is_empty() => (None, "设置中还没有填写“我的名字”，请选择".into()),
        (false, false) => {
            let players = [meta.red.as_ref().map(|_| format!("红方{}", quote(&meta.red))), meta.black.as_ref().map(|_| format!("黑方{}", quote(&meta.black)))];
            (None, format!("{}都不是设置中的名字，请选择", players.into_iter().flatten().collect::<Vec<_>>().join("、")))
        }
    }
}

/// 导入后的名称：双方棋手，或赛事名，或文件名。
fn game_name(meta: &RecordMeta, source: Option<&str>) -> String {
    match (&meta.red, &meta.black) {
        (Some(r), Some(b)) => format!("{r} 对 {b}"),
        _ => meta.event.clone().or_else(|| source.map(str::to_string)).unwrap_or_else(|| "导入的棋谱".into()),
    }
}

/// 日期的排序键（年月日）：认“2026.10.01”“2026-10-1”“2026年10月1日”等写法，缺少的部分记为 0；认不出年份时为空。
pub fn date_key(date: &str) -> Option<u32> {
    let mut parts = date.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty());
    let year: u32 = parts.next().filter(|y| y.len() == 4)?.parse().ok()?;
    let mut rest = parts.map(|p| p.parse::<u32>().unwrap_or(0));
    let month = rest.next().filter(|m| *m <= 12).unwrap_or(0);
    let day = rest.next().filter(|d| *d <= 31).unwrap_or(0);
    Some(year * 10_000 + month * 100 + day)
}

/// 收件箱的排列：有日期的在前，新的在前；没有日期的按导入顺序。
fn sort_entries(entries: &mut [InboxEntry]) {
    entries.sort_by(|a, b| {
        let (ka, kb) = (a.date.as_deref().and_then(date_key), b.date.as_deref().and_then(date_key));
        match (ka, kb) {
            (Some(x), Some(y)) => y.cmp(&x).then(a.added_at.cmp(&b.added_at)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.added_at.cmp(&b.added_at),
        }
    });
}

/// 一盘棋谱能否导入：解析失败、没有着法或着法过多时给出原因。
fn parse_game(text: &str) -> Result<ParsedRecord, String> {
    let parsed = record::parse_record(text).map_err(|e| e.to_string())?;
    if parsed.moves.is_empty() {
        return Err("这盘棋没有可以分析的着法".into());
    }
    let max = StudyLimits::default().max_nodes;
    if parsed.moves.len() >= max {
        return Err(format!("着法超过 {max} 步"));
    }
    Ok(parsed)
}

fn read_source(path: &str) -> Result<(String, Option<String>), String> {
    let path = Path::new(path);
    let size = std::fs::metadata(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?.len();
    if size > MAX_IMPORT_BYTES {
        return Err(too_large("文件", MAX_IMPORT_BYTES).message);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{} 不是 UTF-8 文本，请先转换编码后再导入", path.display()))?;
    Ok((text, path.file_stem().map(|s| s.to_string_lossy().to_string())))
}

impl Inner {
    /// 已有棋局的指纹与名称。
    fn known_games(&self) -> ApiResult<HashMap<String, String>> {
        Ok(self.store.list_studies()?.into_iter().filter_map(|s| Some((s.fingerprint().ok()?, s.name))).collect())
    }

    /// 为收件箱中的一盘排队：还没有讲解、讲解没有完成或讲解规则已经更新的着法都排详细讲解，空闲优先级。
    fn queue_inbox_game(&self, study: &Study) {
        let records: HashMap<_, _> =
            self.store.explanations_for(&study.id).unwrap_or_default().into_iter().map(|r| (r.node_id, r)).collect();
        for node in study.move_nodes() {
            let finished = records.get(&node.id).is_some_and(|r| {
                !r.pending
                    && r.explanation.as_ref().is_some_and(|e| e.generator_version == xq_core::explain::GENERATOR_VERSION)
                    && r.status() != crate::records::ExplainStatus::Basic
            });
            if !finished {
                self.enqueue(&study.id, node.id, Budget::Quick, Priority::Idle, true);
            }
        }
    }

    /// 启动时为收件箱中还没有分析完的着法补排（不用等打开这盘棋）。
    pub(crate) fn resume_inbox(&self) {
        let entries = match self.store.list_inbox() {
            Ok(e) => e,
            Err(e) => return tracing::warn!("读取收件箱失败，没有补排分析：{e}"),
        };
        for entry in entries {
            if let Ok(study) = self.with_study(&entry.study_id, |s| Ok(s.clone())) {
                self.queue_inbox_game(&study);
            }
        }
    }

    fn emit_inbox_updated(&self) { events::emit(self.events.as_ref(), events::INBOX_UPDATED, &serde_json::json!({})); }
}

impl AppService {
    /// 导入前预览：只读不写。
    pub fn inbox_preview(&self, req: InboxPreviewRequest) -> ApiResult<InboxPreview> {
        let names = self.inner.config_snapshot().player.names;
        let known = self.inner.known_games()?;
        let mut seen: HashMap<String, u32> = HashMap::new();
        let mut games = Vec::new();
        let mut notes = Vec::new();
        let mut over = 0;
        for source in req.sources {
            let (text, file) = match source {
                InboxSource::Text { name, text } => {
                    if text.len() as u64 > MAX_IMPORT_BYTES {
                        notes.push(too_large("粘贴的内容", MAX_IMPORT_BYTES).message);
                        continue;
                    }
                    (text, name)
                }
                InboxSource::File { path } => match read_source(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        notes.push(e);
                        continue;
                    }
                },
            };
            for game in record::split_records(&text) {
                if games.len() >= MAX_GAMES {
                    over += 1;
                    continue;
                }
                let index = games.len() as u32;
                let mut c = InboxCandidate {
                    index,
                    source: file.clone(),
                    name: file.clone().unwrap_or_else(|| "导入的棋谱".into()),
                    red: None,
                    black: None,
                    date: None,
                    result: None,
                    plies: 0,
                    stopped: None,
                    error: None,
                    duplicate: None,
                    side: None,
                    side_reason: String::new(),
                    text: game.trim().to_string(),
                };
                match parse_game(&game) {
                    Err(e) => c.error = Some(e),
                    Ok(parsed) => {
                        let meta = parsed.meta();
                        let fp = parsed.fingerprint();
                        c.duplicate = known
                            .get(&fp)
                            .map(|n| format!("与已有的棋局“{}”相同", short(n)))
                            .or_else(|| seen.get(&fp).map(|i| format!("与这一批中的第 {} 盘相同", i + 1)));
                        seen.entry(fp).or_insert(index);
                        c.plies = parsed.moves.len() as u32;
                        c.stopped = parsed.stopped.as_ref().map(|s| {
                            format!("第 {} 步“{}”无法解析：{}，导入此前的 {} 步", s.ply, s.token, s.reason, parsed.moves.len())
                        });
                        (c.side, c.side_reason) = match_side(&meta, &names);
                        c.name = game_name(&meta, file.as_deref());
                        (c.red, c.black, c.date, c.result) = (meta.red, meta.black, meta.date, meta.result);
                    }
                }
                games.push(c);
            }
        }
        if over > 0 {
            notes.push(format!("一次最多导入 {MAX_GAMES} 盘，另外 {over} 盘没有列出，可以之后再导入"));
        }
        if games.is_empty() && notes.is_empty() {
            notes.push("没有找到棋谱".into());
        }
        Ok(InboxPreview { games, notes })
    }

    /// 导入并加入收件箱：再查一次重复，棋局与收件箱记录在同一个写事务中写入，然后在引擎空闲时逐盘分析。
    pub fn inbox_import(&self, req: InboxImportRequest) -> ApiResult<InboxImportResult> {
        if req.games.len() > MAX_GAMES {
            return Err(format!("一次最多导入 {MAX_GAMES} 盘").into());
        }
        let inner = &self.inner;
        let mut known = inner.known_games()?;
        let mut skipped = Vec::new();
        let mut batch: Vec<(Study, InboxEntry)> = Vec::new();
        let start = now_ms();
        for (i, game) in req.games.into_iter().enumerate() {
            let parsed = match parse_game(&game.text) {
                Ok(p) => p,
                Err(e) => {
                    skipped.push(format!("第 {} 盘：{e}", i + 1));
                    continue;
                }
            };
            let meta = parsed.meta();
            let name = game.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| game_name(&meta, None));
            let fp = parsed.fingerprint();
            if let Some(other) = known.get(&fp) {
                skipped.push(format!("第 {} 盘“{}”与已有的棋局“{}”相同", i + 1, short(&name), short(other)));
                continue;
            }
            let start_pos = StudyStart { pos: parsed.start.clone(), counters: parsed.counters, history_complete: !parsed.custom_start };
            let study = inner.build_study(name.clone(), StudySource::Record, start_pos, None, Some(game.user_side), &parsed.moves)?;
            known.insert(fp, name);
            let entry = InboxEntry {
                study_id: study.id.clone(),
                added_at: start + batch.len() as i64,
                red: meta.red,
                black: meta.black,
                date: meta.date,
                result: meta.result,
                exercises: None,
                key_positions: None,
            };
            batch.push((study, entry));
        }
        inner.store.add_to_inbox(&batch)?;
        for (study, _) in &batch {
            inner.cache_study(study);
            let nodes: Vec<_> = study.move_nodes().map(|n| n.id).collect();
            inner.mark_pending(study, &nodes);
            inner.queue_inbox_game(study);
            inner.emit_study_updated(study, "create", &nodes);
        }
        if !batch.is_empty() {
            inner.emit_inbox_updated();
        }
        let added = batch.len() as u32;
        let mut text = if added > 0 { format!("加入收件箱 {added} 盘，引擎空闲时逐盘分析") } else { "没有加入新的棋局".into() };
        if !skipped.is_empty() {
            text.push_str(&format!("；{} 盘没有导入", skipped.len()));
        }
        text.push('。');
        Ok(InboxImportResult { added, skipped, text })
    }

    /// 收件箱中的棋局与分析进度（会话已经删除的不列出）。
    pub fn inbox_list(&self) -> ApiResult<InboxList> {
        let inner = &self.inner;
        let mut entries = inner.store.list_inbox()?;
        sort_entries(&mut entries);
        let mut items = Vec::new();
        for e in entries {
            let Ok(study) = inner.with_study(&e.study_id, |s| Ok(s.clone())) else { continue };
            let stats = inner.study_view(&study)?.stats;
            let analysed = (stats.detailed + stats.partial + stats.failed).min(stats.moves) as u32;
            let done = stats.moves > 0 && stats.pending == 0 && analysed as usize >= stats.moves;
            let key_positions = match (done, e.key_positions) {
                (true, Some(n)) => Some(n),
                (true, None) => {
                    let n = self.game_report(&study.id)?.key_positions.len() as u32;
                    inner.store.update_inbox(&study.id, |x| x.key_positions.replace(n) != Some(n))?;
                    Some(n)
                }
                (false, cached) => {
                    if cached.is_some() {
                        inner.store.update_inbox(&study.id, |x| x.key_positions.take().is_some())?;
                    }
                    None
                }
            };
            items.push(InboxItem {
                study_id: study.id.clone(),
                name: study.name.clone(),
                user_side: study.user_side,
                red: e.red,
                black: e.black,
                date: e.date,
                result: e.result,
                added_at: e.added_at,
                moves: stats.moves as u32,
                analysed,
                incomplete: (stats.partial + stats.failed) as u32,
                done,
                key_positions,
                exercises: e.exercises,
            });
        }
        let done = items.iter().filter(|i| i.done).count();
        let ready = items.iter().filter(|i| i.done && i.exercises.is_none()).count();
        let text = if items.is_empty() {
            "收件箱是空的".to_string()
        } else {
            let mut t = format!("共 {} 盘，{done} 盘已分析完", items.len());
            if ready > 0 {
                t.push_str(&format!("，其中 {ready} 盘可以出题"));
            }
            t.push('。');
            t
        };
        Ok(InboxList { items, text })
    }

    /// 为已经分析完、还没有出过题的棋局出题，汇总新增了几道；还没有分析完的这次不出题。
    pub fn inbox_generate(&self) -> ApiResult<InboxGenerateResult> {
        let list = self.inbox_list()?;
        let mut r = InboxGenerateResult::default();
        let mut skipped = Vec::new();
        for item in &list.items {
            if !item.done {
                r.waiting += 1;
                continue;
            }
            if item.exercises.is_some() {
                continue;
            }
            let summary = self.inner.generate_exercises(&item.study_id)?;
            r.games += 1;
            (r.threat, r.choice, r.better, r.existing) =
                (r.threat + summary.threat, r.choice + summary.choice, r.better + summary.better, r.existing + summary.existing);
            skipped.extend(summary.skipped.iter().take(1).map(|s| format!("{}：{s}", short(&item.name))));
            let done = InboxExercises { at: now_ms(), added: summary.threat + summary.choice + summary.better, text: summary.text };
            self.inner.store.update_inbox(&item.study_id, |e| {
                e.exercises = Some(done);
                true
            })?;
        }
        let added = r.threat + r.choice + r.better;
        let mut text = if r.games == 0 {
            "没有需要出题的棋局：已经分析完的都出过题了".to_string()
        } else if added == 0 {
            format!("为 {} 盘出题，没有新的题目", r.games)
        } else {
            format!("为 {} 盘出题，新增 {added} 道：找威胁 {}、二选一 {}、找更好的着法 {}", r.games, r.threat, r.choice, r.better)
        };
        if r.existing > 0 {
            text.push_str(&format!("；{} 道已经在复习中", r.existing));
        }
        if r.waiting > 0 {
            text.push_str(&format!("；还有 {} 盘没有分析完，这次没有出题", r.waiting));
        }
        if !skipped.is_empty() {
            text.push_str(&format!("；部分失误没有出唯一答案的题目（例如 {}）", skipped.into_iter().take(2).collect::<Vec<_>>().join("；")));
        }
        text.push('。');
        r.text = text;
        if r.games > 0 {
            self.inner.emit_inbox_updated();
        }
        Ok(r)
    }

    /// 移出收件箱：不再列出，棋局与题目都保留。
    pub fn inbox_remove(&self, r: InboxRef) -> ApiResult<InboxList> {
        if self.inner.store.remove_from_inbox(&r.study_id)? {
            self.inner.emit_inbox_updated();
        }
        self.inbox_list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(red: Option<&str>, black: Option<&str>) -> RecordMeta {
        RecordMeta { red: red.map(Into::into), black: black.map(Into::into), ..Default::default() }
    }

    #[test]
    fn side_follows_the_single_matching_name() {
        let names = vec!["张三".to_string(), "Ｑｍｚ".to_string()];
        assert_eq!(match_side(&meta(Some(" 张 三 "), Some("李四")), &names), (Some(Color::Red), "红方“ 张 三 ”与设置中的名字相同".into()));
        assert_eq!(match_side(&meta(Some("李四"), Some("qmz")), &names).0, Some(Color::Black), "全角与大小写不影响比对");
        assert_eq!(match_side(&meta(Some("张三"), Some("qmz")), &names), (None, "红黑双方的名字都与设置中的名字相同，请选择".into()));
        assert_eq!(match_side(&meta(Some("甲"), Some("乙")), &names), (None, "红方“甲”、黑方“乙”都不是设置中的名字，请选择".into()));
        assert_eq!(match_side(&meta(None, None), &names).1, "棋谱中没有写明双方棋手，请选择");
        assert_eq!(match_side(&meta(Some("甲"), None), &[]).1, "设置中还没有填写“我的名字”，请选择");
    }

    #[test]
    fn dates_sort_newest_first_and_undated_by_import_order() {
        assert_eq!(date_key("2026.10.01"), Some(20261001));
        assert_eq!(date_key("2026年9月3日"), Some(20260903));
        assert_eq!(date_key("2026-??-??"), Some(20260000));
        assert_eq!(date_key("10/01"), None);
        let entry = |id: &str, at: i64, date: Option<&str>| InboxEntry {
            study_id: id.into(),
            added_at: at,
            red: None,
            black: None,
            date: date.map(Into::into),
            result: None,
            exercises: None,
            key_positions: None,
        };
        let mut list = vec![entry("a", 1, None), entry("b", 2, Some("2026.09.01")), entry("c", 3, Some("2026.10.01")), entry("d", 0, None)];
        sort_entries(&mut list);
        assert_eq!(list.iter().map(|e| e.study_id.as_str()).collect::<Vec<_>>(), ["c", "b", "d", "a"]);
    }
}
