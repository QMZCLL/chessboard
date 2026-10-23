//! 本地数据库：学习会话（含棋谱树与标注）、引擎分析记录、讲解记录。
//! 使用嵌入式数据库 redb，值以 JSON 保存，并带有数据版本，便于后续迁移。

use std::path::Path;

use redb::Database;
use redb::ReadableTable;
use redb::TableDefinition;
use serde::Serialize;
use serde::de::DeserializeOwned;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::records::AnalysisRecord;
use crate::records::CloudRecord;
use crate::records::ExplanationRecord;

const META: TableDefinition<&str, &str> = TableDefinition::new("meta");
const STUDIES: TableDefinition<&str, &str> = TableDefinition::new("studies");
const ANALYSES: TableDefinition<&str, &str> = TableDefinition::new("analyses");
const EXPLANATIONS: TableDefinition<&str, &str> = TableDefinition::new("explanations");

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
            let mut t = tx.open_table(EXPLANATIONS)?;
            let start = format!("{id}/");
            let end = format!("{id}0");
            let keys: Vec<String> =
                t.range(start.as_str()..end.as_str())?.filter_map(|r| r.ok()).map(|(k, _)| k.value().to_string()).collect();
            for k in keys {
                t.remove(k.as_str())?;
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

    pub fn delete_explanations(&self, study: &str, nodes: &[NodeId]) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(EXPLANATIONS)?;
            for node in nodes {
                t.remove(explanation_key(study, *node).as_str())?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
