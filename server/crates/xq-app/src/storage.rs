//! 本地数据库：学习会话（含棋谱树与标注）、引擎分析记录、讲解记录、AI 讲解记录与缓存、教练追问、复习卡片。
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
use crate::llm::CoachState;
use crate::llm::ask::AnswerRecord;
use crate::records::AnalysisRecord;
use crate::records::CloudRecord;
use crate::records::ExplanationRecord;
use crate::review::ReviewCard;

const META: TableDefinition<&str, &str> = TableDefinition::new("meta");
const STUDIES: TableDefinition<&str, &str> = TableDefinition::new("studies");
const ANALYSES: TableDefinition<&str, &str> = TableDefinition::new("analyses");
const EXPLANATIONS: TableDefinition<&str, &str> = TableDefinition::new("explanations");
/// 每一着的 AI 讲解状态与最近结果，键为“会话/节点”。
const COACH: TableDefinition<&str, &str> = TableDefinition::new("coach");
/// 通过检查的 AI 讲解，键为“会话/节点/缓存键”；证据、模型或讲解设置相同时直接复用。
const COACH_CACHE: TableDefinition<&str, &str> = TableDefinition::new("coach_cache");
const COACH_USAGE: &str = "coach_usage";
/// 教练追问的状态与最近一次回答，键为“会话/比较起点/两步着法/问题类型”（见 `Question::key`）。
const COACH_ASK: TableDefinition<&str, &str> = TableDefinition::new("coach_ask");
/// 复习卡片，键为“会话/节点”（与讲解记录相同）。卡片是独立的快照，删除会话时保留。
const REVIEWS: TableDefinition<&str, &str> = TableDefinition::new("reviews");

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
            tx.open_table(COACH_ASK)?;
            tx.open_table(REVIEWS)?;
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
            for table in [EXPLANATIONS, COACH, COACH_CACHE, COACH_ASK] {
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

    /// 删除这些节点的讲解、AI 讲解与缓存，以及以它们为比较起点的追问。
    pub fn delete_explanations(&self, study: &str, nodes: &[NodeId]) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(EXPLANATIONS)?;
            let mut coach = tx.open_table(COACH)?;
            let mut cache = tx.open_table(COACH_CACHE)?;
            let mut asks = tx.open_table(COACH_ASK)?;
            for node in nodes {
                let key = explanation_key(study, *node);
                t.remove(key.as_str())?;
                coach.remove(key.as_str())?;
                remove_prefix(&mut cache, &format!("{key}/"))?;
                remove_prefix(&mut asks, &format!("{key}/"))?;
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

    /// 卡片还不存在时写入；已经存在时不覆盖，返回已有的卡片。读取与写入在同一写事务中完成。
    pub fn insert_review(&self, card: &ReviewCard) -> Result<ReviewCard, StoreError> {
        let tx = self.db.begin_write()?;
        let stored = {
            let mut t = tx.open_table(REVIEWS)?;
            let existing = t.get(card.id.as_str())?.map(|v| serde_json::from_str::<ReviewCard>(v.value())).transpose()?;
            match existing {
                Some(existing) => existing,
                None => {
                    t.insert(card.id.as_str(), serde_json::to_string(card)?.as_str())?;
                    card.clone()
                }
            }
        };
        tx.commit()?;
        Ok(stored)
    }

    /// 修改一张卡片：在同一写事务中读取最新的卡片、修改并写回，同时把修改次数加一。写事务一次只有一个，
    /// 并发的修改（笔记、状态、追加复习记录、补充讲解）依次进行，不会用旧快照覆盖彼此；修改次数也就随写入顺序递增。
    /// 卡片不存在时返回 `None`。
    pub fn update_review(&self, id: &str, f: impl FnOnce(&mut ReviewCard)) -> Result<Option<ReviewCard>, StoreError> {
        let tx = self.db.begin_write()?;
        let updated = {
            let mut t = tx.open_table(REVIEWS)?;
            let current = t.get(id)?.map(|v| serde_json::from_str::<ReviewCard>(v.value())).transpose()?;
            match current {
                Some(mut card) => {
                    f(&mut card);
                    card.revision += 1;
                    t.insert(id, serde_json::to_string(&card)?.as_str())?;
                    Some(card)
                }
                None => None,
            }
        };
        tx.commit()?;
        Ok(updated)
    }

    pub fn load_review(&self, id: &str) -> Result<Option<ReviewCard>, StoreError> { self.get(REVIEWS, id) }

    pub fn list_reviews(&self) -> Result<Vec<ReviewCard>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(REVIEWS)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (_, v) = item?;
            match serde_json::from_str::<ReviewCard>(v.value()) {
                Ok(c) => out.push(c),
                Err(e) => tracing::warn!("跳过无法解析的复习卡片：{e}"),
            }
        }
        Ok(out)
    }

    pub fn delete_review(&self, id: &str) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        tx.open_table(REVIEWS)?.remove(id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn save_answer(&self, record: &AnswerRecord) -> Result<(), StoreError> {
        self.put(COACH_ASK, &record.question.key(), record)
    }

    pub fn load_answer(&self, key: &str) -> Result<Option<AnswerRecord>, StoreError> { self.get(COACH_ASK, key) }

    /// 把上次运行遗留的排队中、生成中的追问标为中断（保留已有回答），返回处理的条数。
    pub fn reset_interrupted_asks(&self, now: i64) -> Result<usize, StoreError> {
        let tx = self.db.begin_write()?;
        let mut count = 0;
        {
            let mut t = tx.open_table(COACH_ASK)?;
            let mut updates = Vec::new();
            for item in t.iter()? {
                let (k, v) = item?;
                let Ok(mut record) = serde_json::from_str::<AnswerRecord>(v.value()) else { continue };
                if matches!(record.state, CoachState::Queued | CoachState::Running) {
                    record.state = CoachState::Idle;
                    record.error = Some("上次提问在应用退出前没有完成，可以重新提问".into());
                    record.updated_at = now;
                    updates.push((k.value().to_string(), serde_json::to_string(&record)?));
                }
            }
            for (k, v) in updates {
                t.insert(k.as_str(), v.as_str())?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// 把上次运行遗留的排队中、生成中的 AI 讲解标为中断（保留已有结果），返回处理的条数。
    pub fn reset_interrupted_coach(&self, now: i64) -> Result<usize, StoreError> {
        let tx = self.db.begin_write()?;
        let mut count = 0;
        {
            let mut t = tx.open_table(COACH)?;
            let mut updates = Vec::new();
            for item in t.iter()? {
                let (k, v) = item?;
                let Ok(mut record) = serde_json::from_str::<CoachRecord>(v.value()) else { continue };
                if matches!(record.state, CoachState::Queued | CoachState::Running) {
                    record.state = CoachState::Idle;
                    record.waiting_evidence = false;
                    record.error = Some("上次生成在应用退出前没有完成，可以重新生成".into());
                    record.updated_at = now;
                    updates.push((k.value().to_string(), serde_json::to_string(&record)?));
                }
            }
            for (k, v) in updates {
                t.insert(k.as_str(), v.as_str())?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::review::ReviewStatus;

    fn card(id: &str) -> ReviewCard {
        serde_json::from_value(serde_json::json!({
            "id": id, "study_id": "s", "node_id": 1, "study_name": "测试", "created_at": 1, "updated_at": 1,
            "status": "due", "fen": "4k4/9/9/9/9/9/9/9/9/4K4 w", "side": "red", "orientation": "red_bottom",
            "turn": 1, "history": [], "history_complete": true, "answer": { "mv": "e0e1", "notation": "帅五进一" },
            "thinking": null, "basis": null, "note": "", "attempts": [], "reviewed_at": null
        }))
        .unwrap()
    }

    #[test]
    fn concurrent_card_updates_do_not_overwrite_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("db.redb")).unwrap());
        store.insert_review(&card("s/1")).unwrap();
        // 保存笔记的修改读到卡片后停一会儿；同时标记状态。状态的修改必须读到笔记写入之后的卡片
        let (inside, wait_inside) = mpsc::channel();
        let notes = {
            let store = store.clone();
            std::thread::spawn(move || {
                store
                    .update_review("s/1", |c| {
                        inside.send(()).unwrap();
                        std::thread::sleep(Duration::from_millis(300));
                        c.note = "新的理解".into();
                    })
                    .unwrap()
            })
        };
        wait_inside.recv().unwrap();
        store.update_review("s/1", |c| c.status = ReviewStatus::Understood).unwrap();
        notes.join().unwrap();
        let saved = store.load_review("s/1").unwrap().unwrap();
        assert_eq!((saved.note.as_str(), saved.status), ("新的理解", ReviewStatus::Understood));
        // 早期没有修改次数的卡片从 0 算起，两次修改之后为 2
        assert_eq!(saved.revision, 2);
    }

    #[test]
    fn inserting_an_existing_card_keeps_the_stored_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.redb")).unwrap();
        store.insert_review(&card("s/1")).unwrap();
        store.update_review("s/1", |c| c.note = "已有的笔记".into()).unwrap();
        let again = store.insert_review(&card("s/1")).unwrap();
        assert_eq!(again.note, "已有的笔记");
        assert!(store.update_review("s/2", |c| c.note = "x".into()).unwrap().is_none(), "不存在的卡片不会被建立");
    }

    #[test]
    fn interrupted_questions_are_reset_and_keep_their_answers() {
        use crate::llm::CoachState;
        use crate::llm::ask::AnswerRecord;
        use crate::llm::ask::Question;
        use crate::llm::ask::QuestionKind;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.redb")).unwrap();
        let mv = |s: &str| xq_core::Move::parse_iccs(s).unwrap();
        let question = Question { study_id: "s".into(), node_id: 0, mine: mv("h2e2"), reference: mv("h0g2"), kind: QuestionKind::MyMove };
        let record = AnswerRecord { question: question.clone(), state: CoachState::Running, generation: 3, result: None, error: None, updated_at: 1 };
        store.save_answer(&record).unwrap();
        assert_eq!(store.reset_interrupted_asks(9).unwrap(), 1);
        let reset = store.load_answer(&question.key()).unwrap().unwrap();
        assert_eq!(reset.state, CoachState::Idle);
        assert!(reset.error.unwrap().contains("没有完成"));
        // 删除比较起点时一起删除
        store.delete_explanations("s", &[0]).unwrap();
        assert!(store.load_answer(&question.key()).unwrap().is_none());
    }

}
