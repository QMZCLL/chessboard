//! 数据库的版本与迁移。
//!
//! - 版本 1：0.3.0 的格式。0.4 开发期间加了收件箱表与几个可以缺省的字段，仍记为版本 1。
//! - 版本 2：每条记录都按当前的格式写回（以前读取时才补上默认值的字段写成明确的值），并检查过能否读取、内容是否成立。
//!   - 无法读取或不成立的记录原样移到隔离表 `quarantine`（写明原来的表、键与原因），不丢弃，也不让启动失败；
//!   - 讲解、AI 讲解、AI 讲解缓存与追问中，所属会话或着法已经不存在的记录删除（早期删除着法时没有一起删掉）；
//!     原会话已经删除的复习卡片照常保留（卡片是独立的快照）。
//!
//! 迁移在一个写事务中完成：中途出错或进程终止时，数据库保持迁移前的内容，下次启动重新迁移。
//! 迁移前先把数据库文件复制到同一目录下 `backups/xqlink-schema<原版本>-<时间>.redb`。
//! 每次迁移记在 meta 表的 `migrations` 中（从哪个版本到哪个版本、时间、应用版本与处理的条数）。

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use redb::Database;
use redb::ReadableTable;
use redb::TableDefinition;
use redb::WriteTransaction;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use xq_core::study::NodeId;
use xq_core::study::Study;
use xq_core::study::StudyLimits;

use crate::failpoints;
use crate::inbox::InboxEntry;
use crate::learning::LearningEvent;
use crate::llm::CoachRecord;
use crate::llm::CoachResult;
use crate::llm::ask::AnswerRecord;
use crate::practice::PracticeSession;
use crate::records::AnalysisRecord;
use crate::records::CloudRecord;
use crate::records::ExplanationRecord;
use crate::review::ReviewCard;
use crate::storage::ANALYSES;
use crate::storage::COACH;
use crate::storage::COACH_ASK;
use crate::storage::COACH_CACHE;
use crate::storage::EVENTS;
use crate::storage::EXPLANATIONS;
use crate::storage::INBOX;
use crate::storage::META;
use crate::storage::PRACTICE;
use crate::storage::QUARANTINE;
use crate::storage::REVIEWS;
use crate::storage::STUDIES;
use crate::storage::StoreError;

pub const SCHEMA_VERSION: u32 = 2;
const MIGRATIONS_KEY: &str = "migrations";

/// meta 表中记下的一次迁移。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationEntry {
    pub from: u32,
    pub to: u32,
    pub at: i64,
    pub app_version: String,
    /// 按当前格式写回的记录数。
    pub rewritten: usize,
    /// 移到隔离表的记录数。
    pub quarantined: usize,
    /// 删除的、所属着法已经不存在的记录数。
    pub removed: usize,
}

/// 这次启动时做的迁移，由服务转告界面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub entry: MigrationEntry,
    /// 迁移前的数据库副本。
    pub backup: PathBuf,
}

/// 隔离表中的一条记录：无法读取或不成立，原样保存。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quarantined {
    pub table: String,
    pub key: String,
    pub raw: String,
    pub reason: String,
    pub at: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// 数据库中记下的版本；新建的空数据库没有版本。
pub(crate) fn stored_version(db: &Database) -> Result<Option<u32>, StoreError> {
    let tx = db.begin_read()?;
    let meta = match tx.open_table(META) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let Some(v) = meta.get("schema_version")? else { return Ok(None) };
    let text = v.value().to_string();
    text.parse::<u32>().map(Some).map_err(|_| StoreError(format!("无法识别数据库记下的版本“{text}”，没有打开")))
}

/// 已经做过的迁移。
pub(crate) fn history(db: &Database) -> Result<Vec<MigrationEntry>, StoreError> {
    let tx = db.begin_read()?;
    let meta = tx.open_table(META)?;
    let entries = match meta.get(MIGRATIONS_KEY)? {
        Some(v) => serde_json::from_str(v.value())?,
        None => Vec::new(),
    };
    Ok(entries)
}

/// 迁移前复制数据库文件（先写临时文件并刷到磁盘，再改名）。数据库此时不能处于打开状态。
pub(crate) fn backup_copy(path: &Path, from: u32) -> Result<PathBuf, StoreError> {
    let dir = path.parent().map(|d| d.join("backups")).unwrap_or_else(|| PathBuf::from("backups"));
    std::fs::create_dir_all(&dir).map_err(|e| StoreError(format!("无法建立备份目录 {}：{e}", dir.display())))?;
    let target = dir.join(format!("xqlink-schema{from}-{}.redb", now_ms()));
    let data = std::fs::read(path).map_err(|e| StoreError(format!("读取 {} 失败：{e}", path.display())))?;
    crate::files::write_atomic(&target, &data).map_err(|e| StoreError(format!("备份数据库到 {} 失败：{e}", target.display())))?;
    failpoints::hit("migrate:after_backup")?;
    Ok(target)
}

/// 新数据库：建立全部的表并记下当前版本。
pub(crate) fn init(tx: &WriteTransaction) -> Result<(), StoreError> {
    tx.open_table(META)?.insert("schema_version", SCHEMA_VERSION.to_string().as_str())?;
    open_tables(tx)
}

pub(crate) fn open_tables(tx: &WriteTransaction) -> Result<(), StoreError> {
    for t in [STUDIES, ANALYSES, EXPLANATIONS, COACH, COACH_CACHE, COACH_ASK, REVIEWS, PRACTICE, EVENTS, INBOX, QUARANTINE] {
        tx.open_table(t)?;
    }
    Ok(())
}

/// 把数据库从 `from` 迁移到当前版本（在调用者的写事务中，调用者提交）。
pub(crate) fn migrate(tx: &WriteTransaction, from: u32) -> Result<MigrationEntry, StoreError> {
    open_tables(tx)?;
    let mut step = Step { tx, now: now_ms(), rewritten: 0, quarantined: 0, removed: 0 };
    let mut version = from;
    while version < SCHEMA_VERSION {
        match version {
            // 版本 1 与 0 的内容相同（0 是早期没有记下版本的库）
            0 | 1 => step.upgrade_to_v2()?,
            v => return Err(StoreError(format!("没有从版本 {v} 迁移的方法"))),
        }
        version = if version == 0 { 2 } else { version + 1 };
    }
    let entry = MigrationEntry {
        from,
        to: SCHEMA_VERSION,
        at: step.now,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        rewritten: step.rewritten,
        quarantined: step.quarantined,
        removed: step.removed,
    };
    let mut meta = tx.open_table(META)?;
    let mut entries: Vec<MigrationEntry> = match meta.get(MIGRATIONS_KEY)? {
        Some(v) => serde_json::from_str(v.value())?,
        None => Vec::new(),
    };
    entries.push(entry.clone());
    meta.insert(MIGRATIONS_KEY, serde_json::to_string(&entries)?.as_str())?;
    meta.insert("schema_version", SCHEMA_VERSION.to_string().as_str())?;
    drop(meta);
    failpoints::hit("migrate:before_commit")?;
    Ok(entry)
}

/// 一条记录的检查结果。
enum Verdict {
    Keep,
    /// 所属的会话或着法已经不存在：删除。
    Orphan,
    /// 不成立：移到隔离表。
    Invalid(String),
}

struct Step<'a> {
    tx: &'a WriteTransaction,
    now: i64,
    rewritten: usize,
    quarantined: usize,
    removed: usize,
}

/// 讲解、AI 讲解与追问的键以“会话/十位的节点编号”开头。
fn key_node(key: &str) -> Option<(&str, NodeId)> {
    let (study, rest) = key.split_once('/')?;
    let digits = rest.get(..10)?;
    if rest.len() > 10 && !rest[10..].starts_with('/') {
        return None;
    }
    Some((study, digits.parse().ok()?))
}

impl Step<'_> {
    fn entries(&self, table: TableDefinition<&str, &str>) -> Result<Vec<(String, String)>, StoreError> {
        let t = self.tx.open_table(table)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (k, v) = item?;
            out.push((k.value().to_string(), v.value().to_string()));
        }
        Ok(out)
    }

    fn quarantine(&mut self, table: &str, key: &str, raw: &str, reason: String) -> Result<(), StoreError> {
        let item = Quarantined { table: table.into(), key: key.into(), raw: raw.into(), reason, at: self.now };
        tracing::warn!("迁移时把无法使用的记录移到隔离表：{table}/{key}：{}", item.reason);
        self.tx.open_table(QUARANTINE)?.insert(format!("{table}/{key}").as_str(), serde_json::to_string(&item)?.as_str())?;
        self.quarantined += 1;
        Ok(())
    }

    /// 逐条读取、检查并按当前格式写回 `table` 中的记录。
    fn rewrite<T: Serialize + DeserializeOwned>(
        &mut self, table: TableDefinition<&str, &str>, name: &str, mut check: impl FnMut(&str, &T) -> Verdict,
    ) -> Result<(), StoreError> {
        for (key, raw) in self.entries(table)? {
            let verdict = match serde_json::from_str::<T>(&raw) {
                Err(e) => Err(format!("无法读取：{e}")),
                Ok(value) => match check(&key, &value) {
                    Verdict::Keep => Ok(Some(serde_json::to_string(&value)?)),
                    Verdict::Orphan => Ok(None),
                    Verdict::Invalid(reason) => Err(reason),
                },
            };
            let mut t = self.tx.open_table(table)?;
            match verdict {
                Ok(Some(json)) => {
                    t.insert(key.as_str(), json.as_str())?;
                    self.rewritten += 1;
                }
                Ok(None) => {
                    t.remove(key.as_str())?;
                    self.removed += 1;
                }
                Err(reason) => {
                    t.remove(key.as_str())?;
                    drop(t);
                    self.quarantine(name, &key, &raw, reason)?;
                }
            }
        }
        Ok(())
    }

    fn upgrade_to_v2(&mut self) -> Result<(), StoreError> {
        let limits = StudyLimits::default();
        let mut nodes: HashMap<String, HashSet<NodeId>> = HashMap::new();
        self.rewrite::<Study>(STUDIES, "studies", |key, study| {
            if study.id != key {
                return Verdict::Invalid(format!("会话编号 {} 与键不一致", study.id));
            }
            match study.check_integrity(&limits) {
                Ok(()) => {
                    nodes.insert(study.id.clone(), study.nodes.keys().copied().collect());
                    Verdict::Keep
                }
                Err(issues) => Verdict::Invalid(issues.iter().map(ToString::to_string).collect::<Vec<_>>().join("；")),
            }
        })?;
        failpoints::hit("migrate:after_studies")?;
        // 着法是否还在（讲解与 AI 讲解只属于有着法的节点，追问的比较起点可以是起始局面）
        let alive = |key: &str| key_node(key).is_some_and(|(s, n)| nodes.get(s).is_some_and(|set| set.contains(&n)));
        let attached = |key: &str, study: &str, node: NodeId| -> Verdict {
            if !alive(key) {
                Verdict::Orphan
            } else if key_node(key) != Some((study, node)) {
                Verdict::Invalid("记录中的会话与节点和键不一致".into())
            } else {
                Verdict::Keep
            }
        };
        self.rewrite::<ExplanationRecord>(EXPLANATIONS, "explanations", |k, r| attached(k, &r.study_id, r.node_id))?;
        self.rewrite::<CoachRecord>(COACH, "coach", |k, r| attached(k, &r.study_id, r.node_id))?;
        self.rewrite::<CoachResult>(COACH_CACHE, "coach_cache", |k, _| if alive(k) { Verdict::Keep } else { Verdict::Orphan })?;
        self.rewrite::<AnswerRecord>(COACH_ASK, "coach_ask", |k, r| {
            attached(k, &r.question.study_id, r.question.node_id)
        })?;
        // 分析缓存：云库记录的键以“cloud:”开头
        for (key, raw) in self.entries(ANALYSES)? {
            let parsed = if key.starts_with("cloud:") {
                serde_json::from_str::<CloudRecord>(&raw).and_then(|r| serde_json::to_string(&r))
            } else {
                serde_json::from_str::<AnalysisRecord>(&raw).and_then(|r| serde_json::to_string(&r))
            };
            match parsed {
                Ok(json) => {
                    self.tx.open_table(ANALYSES)?.insert(key.as_str(), json.as_str())?;
                    self.rewritten += 1;
                }
                Err(e) => {
                    self.tx.open_table(ANALYSES)?.remove(key.as_str())?;
                    self.quarantine("analyses", &key, &raw, format!("无法读取：{e}"))?;
                }
            }
        }
        self.rewrite::<ReviewCard>(REVIEWS, "reviews", |key, card| {
            if card.id != key {
                return Verdict::Invalid(format!("卡片编号 {} 与键不一致", card.id));
            }
            match crate::review::check_card(card, &limits) {
                Ok(()) => Verdict::Keep,
                Err(reason) => Verdict::Invalid(reason),
            }
        })?;
        self.rewrite::<PracticeSession>(PRACTICE, "practice", |key, s| {
            if s.id == key { Verdict::Keep } else { Verdict::Invalid(format!("练习编号 {} 与键不一致", s.id)) }
        })?;
        self.rewrite::<LearningEvent>(EVENTS, "learning_events", |key, e| {
            if e.id != key {
                return Verdict::Invalid(format!("事件编号 {} 与键不一致", e.id));
            }
            match crate::learning::check_event(e) {
                Ok(()) => Verdict::Keep,
                Err(reason) => Verdict::Invalid(reason),
            }
        })?;
        self.rewrite::<InboxEntry>(INBOX, "inbox", |key, entry| {
            if entry.study_id != key {
                Verdict::Invalid(format!("收件箱记录的会话 {} 与键不一致", entry.study_id))
            } else if !nodes.contains_key(key) {
                Verdict::Orphan
            } else {
                Verdict::Keep
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keys_name_their_study_and_node() {
        assert_eq!(key_node("abc/0000000012"), Some(("abc", 12)));
        assert_eq!(key_node("abc/0000000012/k"), Some(("abc", 12)));
        assert_eq!(key_node("abc/0000000012-x"), None);
        assert_eq!(key_node("abc/12"), None);
        assert_eq!(key_node("abc"), None);
    }
}
