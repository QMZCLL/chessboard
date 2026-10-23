//! 本地数据库：学习会话（含棋谱树与标注）、引擎分析记录、讲解记录、AI 讲解记录与缓存。
//! 使用嵌入式数据库 redb，值以 JSON 保存，并带有数据版本，便于后续迁移。

use std::path::Path;

use redb::Database;
use redb::ReadableTable;
use redb::TableDefinition;
use serde::Serialize;
use serde::de::DeserializeOwned;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::llm::CoachRecord;
use crate::llm::CoachResult;
use crate::records::AnalysisRecord;
use crate::records::CloudRecord;
use crate::records::ExplanationRecord;

const META: TableDefinition<&str, &str> = TableDefinition::new("meta");
const STUDIES: TableDefinition<&str, &str> = TableDefinition::new("studies");
const ANALYSES: TableDefinition<&str, &str> = TableDefinition::new("analyses");
const EXPLANATIONS: TableDefinition<&str, &str> = TableDefinition::new("explanations");
/// 每一着的 AI 讲解状态与最近结果，键为“会话/节点”。
const COACH: TableDefinition<&str, &str> = TableDefinition::new("coach");
/// 通过检查的 AI 讲解，键为“会话/节点/缓存键”；证据、模型或讲解设置相同时直接复用。
const COACH_CACHE: TableDefinition<&str, &str> = TableDefinition::new("coach_cache");
const COACH_USAGE: &str = "coach_usage";

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "数据存储错误：{}", self.0) }
}

impl std::error::Error for StoreError {}

macro_rules! impl_from {
    ($($t:ty),*) => {$(
        impl From<$t> for StoreError {
            fn from(e: $t) -> Self { StoreError(e.to_string()) }
        }
    )*};
}
impl_from!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
    serde_json::Error
);

pub struct Store {
    db: Database,
}

fn explanation_key(study: &str, node: NodeId) -> String { format!("{study}/{node:010}") }

fn coach_cache_key(study: &str, node: NodeId, key: &str) -> String { format!("{study}/{node:010}/{key}") }

/// 删除以 `prefix` 开头的所有键（`prefix` 以“/”结尾，“0”是“/”之后的下一个字符）。
fn remove_prefix(t: &mut redb::Table<&str, &str>, prefix: &str) -> Result<(), StoreError> {
    let end = format!("{}0", prefix.trim_end_matches('/'));
    let keys: Vec<String> =
        t.range(prefix..end.as_str())?.filter_map(|r| r.ok()).map(|(k, _)| k.value().to_string()).collect();
    for k in keys {
        t.remove(k.as_str())?;
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| StoreError(e.to_string()))?;
        }
        let db = Database::create(path)?;
        let tx = db.begin_write()?;
        {
            let mut meta = tx.open_table(META)?;
            let version = meta.get("schema_version")?.map(|v| v.value().parse::<u32>().unwrap_or(0));
            match version {
                None => {
                    meta.insert("schema_version", SCHEMA_VERSION.to_string().as_str())?;
                }
                Some(v) if v > SCHEMA_VERSION => {
                    return Err(StoreError(format!("数据库来自更新的版本（{v}），请升级应用后再打开")));
                }
                Some(_) => {}
            }
            tx.open_table(STUDIES)?;
            tx.open_table(ANALYSES)?;
            tx.open_table(EXPLANATIONS)?;
            tx.open_table(COACH)?;
            tx.open_table(COACH_CACHE)?;
        }
        tx.commit()?;
        Ok(Store { db })
    }

    fn put<T: Serialize>(&self, table: TableDefinition<&str, &str>, key: &str, value: &T) -> Result<(), StoreError> {
        let json = serde_json::to_string(value)?;
        let tx = self.db.begin_write()?;
        tx.open_table(table)?.insert(key, json.as_str())?;
        tx.commit()?;
        Ok(())
    }

    fn get<T: DeserializeOwned>(&self, table: TableDefinition<&str, &str>, key: &str) -> Result<Option<T>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(table)?;
        match t.get(key)? {
            Some(v) => Ok(Some(serde_json::from_str(v.value())?)),
            None => Ok(None),
        }
    }

    pub fn save_study(&self, study: &Study) -> Result<(), StoreError> { self.put(STUDIES, &study.id, study) }

    pub fn load_study(&self, id: &str) -> Result<Option<Study>, StoreError> { self.get(STUDIES, id) }

    pub fn list_studies(&self) -> Result<Vec<Study>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(STUDIES)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (_, v) = item?;
            match serde_json::from_str::<Study>(v.value()) {
                Ok(s) => out.push(s),
                Err(e) => tracing::warn!("跳过无法解析的学习会话：{e}"),
            }
        }
        Ok(out)
    }

    pub fn delete_study(&self, id: &str) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        {
            tx.open_table(STUDIES)?.remove(id)?;
            let prefix = format!("{id}/");
            for table in [EXPLANATIONS, COACH, COACH_CACHE] {
                remove_prefix(&mut tx.open_table(table)?, &prefix)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save_analysis(&self, record: &AnalysisRecord) -> Result<(), StoreError> {
        self.put(ANALYSES, &record.key, record)
    }

    pub fn load_analysis(&self, key: &str) -> Result<Option<AnalysisRecord>, StoreError> { self.get(ANALYSES, key) }

    pub fn save_cloud(&self, record: &CloudRecord) -> Result<(), StoreError> {
        self.put(ANALYSES, &format!("cloud:{}", record.fen), record)
    }

    pub fn load_cloud(&self, fen: &str) -> Result<Option<CloudRecord>, StoreError> {
        self.get(ANALYSES, &format!("cloud:{fen}"))
    }

    pub fn save_explanation(&self, record: &ExplanationRecord) -> Result<(), StoreError> {
        self.put(EXPLANATIONS, &explanation_key(&record.study_id, record.node_id), record)
    }

    pub fn load_explanation(&self, study: &str, node: NodeId) -> Result<Option<ExplanationRecord>, StoreError> {
        self.get(EXPLANATIONS, &explanation_key(study, node))
    }

    pub fn explanations_for(&self, study: &str) -> Result<Vec<ExplanationRecord>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(EXPLANATIONS)?;
        let start = format!("{study}/");
        let end = format!("{study}0");
        let mut out = Vec::new();
        for item in t.range(start.as_str()..end.as_str())? {
            let (_, v) = item?;
            out.push(serde_json::from_str(v.value())?);
        }
        Ok(out)
    }

    /// 删除这些节点的讲解、AI 讲解与缓存。
    pub fn delete_explanations(&self, study: &str, nodes: &[NodeId]) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(EXPLANATIONS)?;
            let mut coach = tx.open_table(COACH)?;
            let mut cache = tx.open_table(COACH_CACHE)?;
            for node in nodes {
                let key = explanation_key(study, *node);
                t.remove(key.as_str())?;
                coach.remove(key.as_str())?;
                remove_prefix(&mut cache, &format!("{key}/"))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save_coach(&self, record: &CoachRecord) -> Result<(), StoreError> {
        self.put(COACH, &explanation_key(&record.study_id, record.node_id), record)
    }

    pub fn load_coach(&self, study: &str, node: NodeId) -> Result<Option<CoachRecord>, StoreError> {
        self.get(COACH, &explanation_key(study, node))
    }

    pub fn save_coach_cache(&self, study: &str, node: NodeId, result: &CoachResult) -> Result<(), StoreError> {
        self.put(COACH_CACHE, &coach_cache_key(study, node, &result.cache_key), result)
    }

    pub fn load_coach_cache(&self, study: &str, node: NodeId, key: &str) -> Result<Option<CoachResult>, StoreError> {
        self.get(COACH_CACHE, &coach_cache_key(study, node, key))
    }

    /// 某天（UTC 日序号）已经发出的 AI 讲解请求数。
    pub fn coach_usage(&self, day: i64) -> Result<u32, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(META)?;
        Ok(t.get(COACH_USAGE)?.map(|v| parse_usage(v.value(), day)).unwrap_or(0))
    }

    /// 在预算内记一次调用；已达到上限（`limit` 为 0 表示不限）时返回 false。读取与写入在同一事务中完成。
    pub fn take_coach_budget(&self, day: i64, limit: u32) -> Result<bool, StoreError> {
        let tx = self.db.begin_write()?;
        let allowed = {
            let mut t = tx.open_table(META)?;
            let used = t.get(COACH_USAGE)?.map(|v| parse_usage(v.value(), day)).unwrap_or(0);
            let allowed = limit == 0 || used < limit;
            if allowed {
                t.insert(COACH_USAGE, format!("{day}:{}", used + 1).as_str())?;
            }
            allowed
        };
        tx.commit()?;
        Ok(allowed)
    }
}

fn parse_usage(value: &str, day: i64) -> u32 {
    match value.split_once(':') {
        Some((d, n)) if d.parse::<i64>().ok() == Some(day) => n.parse().unwrap_or(0),
        _ => 0,
    }
}
