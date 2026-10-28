//! 学习档案：全部学习会话（棋谱、分支、标注、练习记录与讲解）与复习卡片（历次复习、意图对照、
//! 局面决策的比较结果、理解与下一次复习的安排），用于换设备恢复与备份。
//!
//! - 导入前先预览：新增与已有的会话，新增、相同与需要合并的卡片；
//! - 会话保留原来的编号，卡片与原局面的关联不变；本机已有同一编号的会话时保留本机的版本；
//! - 卡片按编号合并：历次复习按时间合在一起，状态、理解与安排取较新的一份；原会话已经删除的卡片照样导入，
//!   仍可以复习，只是不能打开原局面；
//! - 同一次复习按它自己的比较结果合并（已完成的优先于进行中的），不依赖整张卡片的更新时间：
//!   比较结果写回时不改卡片的更新时间，同一时刻导出的两份档案可能只差在比较是否完成；
//! - 导入后仍在进行中的比较（导出时还没有完成）在本机重新排队；
//! - 每天第一次启动时自动备份一份（日期按 UTC），保留的份数见设置；也可以立即备份（覆盖当天的一份）。

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tracing::info;
use tracing::warn;
use xq_core::study::Study;

use crate::records::ExplanationRecord;
use crate::decision;
use crate::decision::DecisionState;
use crate::review::MAX_ATTEMPTS;
use crate::review::ReviewAttempt;
use crate::review::ReviewCard;
use crate::review::ReviewStatus;
use crate::schedule;
use crate::service::APP_VERSION;
use crate::service::ApiError;
use crate::service::ApiResult;
use crate::service::AppService;
use crate::service::Inner;
use crate::service::now_ms;

const ARCHIVE_FORMAT: &str = "xqlink-archive";
const ARCHIVE_VERSION: u32 = 1;
const BACKUP_PREFIX: &str = "xqlink-archive-";

#[derive(Serialize, Deserialize)]
struct ArchiveStudy {
    study: Study,
    explanations: Vec<ExplanationRecord>,
}

#[derive(Serialize, Deserialize)]
struct Archive {
    format: String,
    version: u32,
    app_version: String,
    exported_at: i64,
    studies: Vec<ArchiveStudy>,
    reviews: Vec<ReviewCard>,
}

/// 档案的来源：桌面应用传文件路径，浏览器中传文件内容。
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArchiveSource {
    Text { text: String },
    File { path: PathBuf },
}

/// 导入的预览（导入后为实际处理的结果）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ArchiveSummary {
    pub exported_at: i64,
    pub app_version: String,
    pub studies_new: usize,
    /// 本机已有同一编号的会话：保留本机的版本。
    pub studies_existing: usize,
    pub cards_new: usize,
    /// 与本机的卡片内容相同。
    pub cards_same: usize,
    /// 与本机的卡片不同：历次复习合在一起，其他内容取较新的一份。
    pub cards_merge: usize,
    /// 原会话不在本机、也不在档案中的卡片：照样导入，只是不能打开原局面。
    pub cards_orphan: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct BackupFile {
    pub name: String,
    pub size: u64,
    pub modified_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct BackupInfo {
    pub dir: String,
    /// 保留的份数；0 表示不自动备份。
    pub keep: u32,
    /// 已有的备份，新的在前。
    pub files: Vec<BackupFile>,
}

/// 两张同一编号的卡片合并：历次复习按时间合在一起，同一次复习取比较进行得更完整的那份
/// （同样完整时取较新的卡片中的那份）；状态、理解与安排等取更新时间较新的一份；修改次数保留本机的（写入时再加一）。
/// 安排所依据的那次复习因合并有了比较结果时，按结果重新安排（见 `schedule::apply`）。
pub fn merge_cards(local: &ReviewCard, incoming: &ReviewCard, intervals: &[u32]) -> ReviewCard {
    let (newer, older) = if incoming.updated_at > local.updated_at { (incoming, local) } else { (local, incoming) };
    let mut attempts: BTreeMap<i64, ReviewAttempt> = BTreeMap::new();
    for a in older.attempts.iter().chain(newer.attempts.iter()) {
        match attempts.get(&a.at) {
            Some(kept) if decision::progress(kept.decision.as_ref()) > decision::progress(a.decision.as_ref()) => {}
            _ => {
                attempts.insert(a.at, a.clone());
            }
        }
    }
    let mut attempts: Vec<_> = attempts.into_values().collect();
    attempts.drain(..attempts.len().saturating_sub(MAX_ATTEMPTS));
    let mut merged = ReviewCard {
        attempts,
        created_at: local.created_at.min(incoming.created_at),
        reviewed_at: local.reviewed_at.max(incoming.reviewed_at),
        basis: newer.basis.clone().or_else(|| older.basis.clone()),
        thinking: newer.thinking.clone().or_else(|| older.thinking.clone()),
        revision: local.revision,
        ..newer.clone()
    };
    let last = merged.attempts.last().map(|a| a.at);
    let based_on_last = merged.schedule_base.is_some_and(|b| b.attempt_at.is_some() && b.attempt_at == last);
    if based_on_last && merged.status != ReviewStatus::Due {
        // 起点已经存在，`now` 不会用到
        let status = merged.status;
        schedule::apply(&mut merged, status, now_ms(), intervals);
    }
    merged
}

/// 两张卡片内容是否相同（修改次数是各自设备上的计数，不参与比较）。
fn same_card(a: &ReviewCard, b: &ReviewCard) -> bool { ReviewCard { revision: 0, ..a.clone() } == ReviewCard { revision: 0, ..b.clone() } }

/// UTC 日序号对应的日期（年、月、日）。
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn backup_name(now: i64) -> String {
    let (y, m, d) = civil_date(now.div_euclid(86_400_000));
    format!("{BACKUP_PREFIX}{y:04}-{m:02}-{d:02}.json")
}

fn parse_archive(text: &str) -> ApiResult<Archive> {
    let archive: Archive = serde_json::from_str(text.trim()).map_err(|e| format!("学习档案无法解析：{e}"))?;
    if archive.format != ARCHIVE_FORMAT {
        return Err("不是本应用导出的学习档案".into());
    }
    if archive.version > ARCHIVE_VERSION {
        return Err("学习档案来自更新的版本，请升级应用后再导入".into());
    }
    Ok(archive)
}

impl Inner {
    fn build_archive(&self) -> ApiResult<Archive> {
        let mut studies = Vec::new();
        for stored in self.store.list_studies()? {
            // 已经打开的会话以内存中的为准（不为导出而把其他会话载入内存）
            let open = self.studies.lock().unwrap_or_else(|p| p.into_inner()).get(&stored.id).cloned();
            let study = open.unwrap_or(stored);
            let explanations = self.store.explanations_for(&study.id)?;
            studies.push(ArchiveStudy { study, explanations });
        }
        Ok(Archive {
            format: ARCHIVE_FORMAT.into(),
            version: ARCHIVE_VERSION,
            app_version: APP_VERSION.into(),
            exported_at: now_ms(),
            studies,
            reviews: self.store.list_reviews()?,
        })
    }

    fn archive_text(&self) -> ApiResult<String> {
        serde_json::to_string(&self.build_archive()?).map_err(|e| ApiError::from(format!("生成学习档案失败：{e}")))
    }

    fn read_source(&self, source: ArchiveSource) -> ApiResult<Archive> {
        match source {
            ArchiveSource::Text { text } => parse_archive(&text),
            ArchiveSource::File { path } => {
                let text = std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
                parse_archive(&text)
            }
        }
    }

    /// 按本机现有的数据统计导入会怎样处理。
    fn plan(&self, archive: &Archive) -> ApiResult<ArchiveSummary> {
        let local: HashSet<String> = self.store.list_studies()?.into_iter().map(|s| s.id).collect();
        let incoming: HashSet<&str> = archive.studies.iter().map(|s| s.study.id.as_str()).collect();
        let mut summary =
            ArchiveSummary { exported_at: archive.exported_at, app_version: archive.app_version.clone(), ..Default::default() };
        for s in &archive.studies {
            if local.contains(&s.study.id) {
                summary.studies_existing += 1;
            } else {
                summary.studies_new += 1;
            }
        }
        for card in &archive.reviews {
            match self.store.load_review(&card.id)? {
                None => summary.cards_new += 1,
                Some(existing) if same_card(&existing, card) => summary.cards_same += 1,
                Some(_) => summary.cards_merge += 1,
            }
            if !local.contains(&card.study_id) && !incoming.contains(card.study_id.as_str()) {
                summary.cards_orphan += 1;
            }
        }
        Ok(summary)
    }

    fn import(&self, archive: Archive) -> ApiResult<ArchiveSummary> {
        let summary = self.plan(&archive)?;
        for ArchiveStudy { study, explanations } in archive.studies {
            if self.store.load_study(&study.id)?.is_some() {
                continue;
            }
            let mut study = study;
            study.live_node = None;
            self.store.save_study(&study)?;
            for mut record in explanations {
                if record.study_id == study.id && study.nodes.contains_key(&record.node_id) {
                    record.pending = false;
                    self.store.save_explanation(&record)?;
                }
            }
        }
        let intervals = self.config_snapshot().review.intervals_days;
        for card in archive.reviews {
            let mut stored = self.store.insert_review(&card)?;
            if !same_card(&stored, &card)
                && let Some(merged) = self.store.update_review(&card.id, |c| *c = merge_cards(c, &card, &intervals))?
            {
                stored = merged;
            }
            // 导出时还没有完成的比较不会在本机继续：重新排队
            for a in &stored.attempts {
                if let (Some(choice), Some(d)) = (&a.choice, &a.decision)
                    && d.state == DecisionState::Pending
                {
                    self.review_decide(&stored, a.at, choice.mv);
                }
            }
        }
        Ok(summary)
    }

    fn backup_dir(&self) -> PathBuf { self.paths.data_dir.join("backups") }

    /// 写入当天的备份（已有时覆盖），并删除超出保留份数的旧备份。
    fn write_backup(&self, keep: u32) -> ApiResult<()> {
        let dir = self.backup_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("无法建立备份目录：{e}"))?;
        let path = dir.join(backup_name(now_ms()));
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.archive_text()?).map_err(|e| format!("写入备份失败：{e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("写入备份失败：{e}"))?;
        let mut names = backup_names(&dir);
        names.sort_unstable_by(|a, b| b.cmp(a));
        for old in names.iter().skip(keep.max(1) as usize) {
            if let Err(e) = std::fs::remove_file(dir.join(old)) {
                warn!("删除旧备份 {old} 失败：{e}");
            }
        }
        Ok(())
    }

    /// 启动时：当天还没有备份、本机也有学习数据时备份一份。
    pub(crate) fn auto_backup(&self) {
        let keep = self.config_snapshot().review.backup_keep;
        if keep == 0 || self.backup_dir().join(backup_name(now_ms())).exists() {
            return;
        }
        let has_data = self.store.list_studies().is_ok_and(|s| !s.is_empty()) || self.store.list_reviews().is_ok_and(|c| !c.is_empty());
        if !has_data {
            return;
        }
        match self.write_backup(keep) {
            Ok(()) => info!("已备份学习档案到 {}", self.backup_dir().display()),
            Err(e) => warn!("自动备份学习档案失败：{}", e.message),
        }
    }

    fn backup_info(&self) -> BackupInfo {
        let dir = self.backup_dir();
        let mut files: Vec<BackupFile> = backup_names(&dir)
            .into_iter()
            .filter_map(|name| {
                let meta = std::fs::metadata(dir.join(&name)).ok()?;
                let modified_at = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis() as i64);
                Some(BackupFile { name, size: meta.len(), modified_at })
            })
            .collect();
        files.sort_by(|a, b| b.name.cmp(&a.name));
        BackupInfo { dir: dir.display().to_string(), keep: self.config_snapshot().review.backup_keep, files }
    }
}

fn backup_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(".json"))
                .collect()
        })
        .unwrap_or_default()
}

impl AppService {
    /// 导出学习档案（全部会话与复习卡片）。
    pub fn export_archive(&self) -> ApiResult<String> { self.inner.archive_text() }

    pub fn export_archive_to_file(&self, path: &Path) -> ApiResult<()> {
        std::fs::write(path, self.inner.archive_text()?).map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;
        Ok(())
    }

    /// 预览导入：不改动本机的数据。
    pub fn preview_archive(&self, source: ArchiveSource) -> ApiResult<ArchiveSummary> {
        let archive = self.inner.read_source(source)?;
        self.inner.plan(&archive)
    }

    pub fn import_archive(&self, source: ArchiveSource) -> ApiResult<ArchiveSummary> {
        let archive = self.inner.read_source(source)?;
        self.inner.import(archive)
    }

    pub fn backup_info(&self) -> BackupInfo { self.inner.backup_info() }

    /// 立即备份（覆盖当天的一份）。设置为不自动备份时也可以手动备份，只保留最新的这一份。
    pub fn backup_now(&self) -> ApiResult<BackupInfo> {
        let keep = self.inner.config_snapshot().review.backup_keep;
        self.inner.write_backup(keep)?;
        Ok(self.inner.backup_info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::PracticeGoal;
    use crate::decision::DecisionFeedback;
    use crate::decision::DecisionSupport;
    use crate::decision::DecisionVerdict;
    use crate::review::ReviewMove;

    const DAYS: [u32; 5] = [1, 3, 7, 14, 30];

    fn card(updated_at: i64, note: &str, attempts: &[i64]) -> ReviewCard {
        let mut c: ReviewCard = serde_json::from_value(serde_json::json!({
            "id": "s/0000000001", "study_id": "s", "node_id": 1, "study_name": "测试", "created_at": 1, "updated_at": updated_at,
            "status": "due", "fen": "4k4/9/9/9/9/9/9/9/9/4K4 w", "side": "red", "orientation": "red_bottom",
            "turn": 1, "history": [], "history_complete": true, "answer": { "mv": "e0e1", "notation": "帅五进一" },
            "thinking": null, "basis": null, "note": note, "attempts": [], "reviewed_at": null
        }))
        .unwrap();
        c.attempts = attempts
            .iter()
            .map(|at| ReviewAttempt {
                at: *at,
                choice: None,
                matched: false,
                goal: PracticeGoal::Recall,
                intent: None,
                reflection: None,
                decision: None,
            })
            .collect();
        c.reviewed_at = attempts.iter().max().copied();
        c
    }

    #[test]
    fn merging_keeps_every_attempt_and_the_newer_card_otherwise() {
        let mut local = card(100, "本机的理解", &[10, 20]);
        local.revision = 7;
        let mut incoming = card(200, "另一台设备的理解", &[20, 30]);
        incoming.status = ReviewStatus::Understood;
        incoming.due_at = Some(5_000);
        let merged = merge_cards(&local, &incoming, &DAYS);
        assert_eq!(merged.attempts.iter().map(|a| a.at).collect::<Vec<_>>(), [10, 20, 30]);
        assert_eq!((merged.note.as_str(), merged.status, merged.due_at), ("另一台设备的理解", ReviewStatus::Understood, Some(5_000)));
        assert_eq!((merged.reviewed_at, merged.revision), (Some(30), 7), "修改次数保留本机的");
        // 本机的较新：保留本机的内容
        let newer_local = card(300, "本机后来改过的理解", &[40]);
        assert_eq!(merge_cards(&newer_local, &incoming, &DAYS).note, "本机后来改过的理解");
        // 修改次数不同、内容相同：视为相同
        assert!(same_card(&local, &ReviewCard { revision: 1, ..local.clone() }));
        // 合在一起超过保留次数时保留最近的
        let many = card(300, "", &(0..MAX_ATTEMPTS as i64).collect::<Vec<_>>());
        let more = card(10, "", &[1_000, 1_001]);
        let merged = merge_cards(&many, &more, &DAYS);
        assert_eq!(merged.attempts.len(), MAX_ATTEMPTS);
        assert_eq!(merged.attempts.last().unwrap().at, 1_001);
    }

    #[test]
    fn backup_names_use_the_utc_date() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(backup_name(1_761_436_800_000), "xqlink-archive-2025-10-26.json");
        assert_eq!(civil_date(-1), (1969, 12, 31));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
    }

    #[test]
    fn a_finished_comparison_survives_merging_in_either_order() {
        // 同一次复习、同样的卡片更新时间：一份导出时比较还在进行，另一份已经完成
        let decided = |state: DecisionState| {
            let mut c = card(100, "", &[50]);
            let a = &mut c.attempts[0];
            a.goal = PracticeGoal::Decision;
            a.choice = Some(ReviewMove { mv: xq_core::Move::parse_iccs("e0e1").unwrap(), notation: "帅五进一".into() });
            a.decision = Some(DecisionFeedback {
                state,
                verdict: (state == DecisionState::Ready).then_some(DecisionVerdict::Reasonable),
                support: if state == DecisionState::Ready { DecisionSupport::Supported } else { DecisionSupport::Unknown },
                reference: None,
                points: vec![],
                conditions: None,
                error: None,
            });
            // 比较还没有结果时已经标为已理解：先按不升档安排
            c.status = ReviewStatus::Understood;
            crate::schedule::apply(&mut c, ReviewStatus::Understood, 1_000, &DAYS);
            c
        };
        let (pending, ready) = (decided(DecisionState::Pending), decided(DecisionState::Ready));
        assert_eq!((pending.stage, ready.stage), (0, 1));
        for (local, incoming) in [(&pending, &ready), (&ready, &pending)] {
            let merged = merge_cards(local, incoming, &DAYS);
            assert_eq!(merged.attempts[0].decision.as_ref().unwrap().state, DecisionState::Ready, "完成的比较结果保留");
            assert_eq!((merged.stage, merged.due_at), (1, Some(1_000 + 3 * crate::schedule::DAY_MS)), "按结果重新安排");
        }
    }
}
