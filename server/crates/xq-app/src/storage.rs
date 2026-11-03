//! 本地数据库：学习会话（含棋谱树与标注）、引擎分析记录、讲解记录、AI 讲解记录与缓存、教练追问、复习卡片与练习批次、
//! 学习事件与复盘收件箱。
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
use crate::inbox::InboxEntry;
use crate::learning::LearningEvent;
use crate::practice::PracticeSession;
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
/// 练习批次，键为批次编号（见 `practice`）。
const PRACTICE: TableDefinition<&str, &str> = TableDefinition::new("practice");
/// 学习事件，键为事件编号（见 `learning`）。只追加；局面决策的结果晚到时更新同一条。
const EVENTS: TableDefinition<&str, &str> = TableDefinition::new("learning_events");
/// 复盘收件箱中的棋局，键为会话编号（见 `inbox`）。删除会话时一并删除。
const INBOX: TableDefinition<&str, &str> = TableDefinition::new("inbox");
/// 已经从早期的记录补上学习事件（见 `learning`）。
const EVENTS_BACKFILLED: &str = "events_backfilled";

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

/// 带检查的修改的结果。
#[derive(Debug)]
pub enum Checked<T> {
    Written(T),
    /// 不需要写回（或记录不存在）。
    Unchanged,
    /// 检查没有通过，没有写入。
    Rejected(String),
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
            tx.open_table(PRACTICE)?;
            tx.open_table(EVENTS)?;
            tx.open_table(INBOX)?;
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

    /// 在同一写事务中读取最新的值、修改并写回；`f` 返回 `false` 时不写回。不存在或没有写回时返回 `None`。
    fn update_if<T: Serialize + DeserializeOwned>(
        &self, table: TableDefinition<&str, &str>, key: &str, f: impl FnOnce(&mut T) -> bool,
    ) -> Result<Option<T>, StoreError> {
        let tx = self.db.begin_write()?;
        let updated = {
            let mut t = tx.open_table(table)?;
            let current = t.get(key)?.map(|v| serde_json::from_str::<T>(v.value())).transpose()?;
            match current {
                Some(mut value) => {
                    if f(&mut value) {
                        t.insert(key, serde_json::to_string(&value)?.as_str())?;
                        Some(value)
                    } else {
                        None
                    }
                }
                None => None,
            }
        };
        tx.commit()?;
        Ok(updated)
    }

    /// 表中全部能解析的值；无法解析的跳过并记录。
    fn list<T: DeserializeOwned>(&self, table: TableDefinition<&str, &str>, what: &str) -> Result<Vec<T>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(table)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (_, v) = item?;
            match serde_json::from_str::<T>(v.value()) {
                Ok(c) => out.push(c),
                Err(e) => tracing::warn!("跳过无法解析的{what}：{e}"),
            }
        }
        Ok(out)
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
            tx.open_table(INBOX)?.remove(id)?;
            let prefix = format!("{id}/");
            for table in [EXPLANATIONS, COACH, COACH_CACHE, COACH_ASK] {
                remove_prefix(&mut tx.open_table(table)?, &prefix)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 加入复盘收件箱：棋局与收件箱中的记录在同一个写事务中写入，不会只写入其中一部分。
    pub fn add_to_inbox(&self, games: &[(Study, InboxEntry)]) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        {
            let mut studies = tx.open_table(STUDIES)?;
            let mut inbox = tx.open_table(INBOX)?;
            for (study, entry) in games {
                studies.insert(study.id.as_str(), serde_json::to_string(study)?.as_str())?;
                inbox.insert(entry.study_id.as_str(), serde_json::to_string(entry)?.as_str())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_inbox(&self) -> Result<Vec<InboxEntry>, StoreError> { self.list(INBOX, "收件箱记录") }

    pub fn update_inbox(&self, id: &str, f: impl FnOnce(&mut InboxEntry) -> bool) -> Result<Option<InboxEntry>, StoreError> {
        self.update_if(INBOX, id, f)
    }

    /// 移出收件箱，返回是否原来在收件箱中；棋局本身不受影响。
    pub fn remove_from_inbox(&self, id: &str) -> Result<bool, StoreError> {
        let tx = self.db.begin_write()?;
        let removed = tx.open_table(INBOX)?.remove(id)?.is_some();
        tx.commit()?;
        Ok(removed)
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
        self.update_review_if(id, |card| {
            f(card);
            true
        })
    }

    /// 与 `update_review` 相同，但 `f` 在同一写事务中检查最新的卡片，返回 `false` 时不写回（修改次数也不变），
    /// 结果为 `None`。
    pub fn update_review_if(&self, id: &str, f: impl FnOnce(&mut ReviewCard) -> bool) -> Result<Option<ReviewCard>, StoreError> {
        self.update_if(REVIEWS, id, |card: &mut ReviewCard| {
            let write = f(card);
            if write {
                card.revision += 1;
            }
            write
        })
    }

    /// 导入：在同一写事务中写入本机还没有的会话及其讲解，再写入或合并卡片；任何一步失败时什么都不写入。
    /// 本机已有同一编号的会话时保留本机的版本（连同它的讲解）。`merge(本机, 导入)` 返回合并后的卡片，
    /// 不需要改动时返回 `None`；写回时修改次数加一。返回每张卡片写入后的内容。
    /// 本机的卡片无法解析时按不存在处理（列表中已经读不到它），由导入的卡片代替。
    ///
    /// 学习事件同一编号已有时不覆盖。
    pub fn import_batch(
        &self, studies: &[(Study, Vec<ExplanationRecord>)], cards: &[ReviewCard], events: &[LearningEvent],
        merge: impl Fn(&ReviewCard, &ReviewCard) -> Option<ReviewCard>,
    ) -> Result<Vec<ReviewCard>, StoreError> {
        let tx = self.db.begin_write()?;
        let stored = {
            let mut saved = tx.open_table(STUDIES)?;
            let mut explanations = tx.open_table(EXPLANATIONS)?;
            for (study, records) in studies {
                if saved.get(study.id.as_str())?.is_some() {
                    continue;
                }
                saved.insert(study.id.as_str(), serde_json::to_string(study)?.as_str())?;
                for r in records {
                    explanations.insert(explanation_key(&r.study_id, r.node_id).as_str(), serde_json::to_string(r)?.as_str())?;
                }
            }
            let mut reviews = tx.open_table(REVIEWS)?;
            let mut out = Vec::with_capacity(cards.len());
            for card in cards {
                let local = match reviews.get(card.id.as_str())?.map(|v| serde_json::from_str::<ReviewCard>(v.value())) {
                    Some(Ok(c)) => Some(c),
                    Some(Err(e)) => {
                        tracing::warn!("本机的卡片 {} 无法解析，由导入的卡片代替：{e}", card.id);
                        None
                    }
                    None => None,
                };
                let write = match &local {
                    None => Some(card.clone()),
                    Some(l) => merge(l, card).map(|m| ReviewCard { revision: l.revision + 1, ..m }),
                };
                match write {
                    Some(c) => {
                        reviews.insert(c.id.as_str(), serde_json::to_string(&c)?.as_str())?;
                        out.push(c);
                    }
                    None => out.extend(local),
                }
            }
            insert_events(&mut tx.open_table(EVENTS)?, events)?;
            out
        };
        tx.commit()?;
        Ok(stored)
    }

    /// 记一次作答：在同一写事务中读取最新的卡片与它所属的练习批次（给出 `practice` 时；不存在时为 `None`），
    /// 由 `f` 检查并修改卡片。`f` 返回 `Ok(false)` 时不写回，返回错误时什么都不写入。卡片不存在时为 `Unchanged`。
    ///
    /// 写回时 `event` 给出这次作答的学习事件，在同一个写事务中写入（同一编号已有时不覆盖）。
    pub fn update_review_in_practice(
        &self, id: &str, practice: Option<&str>, f: impl FnOnce(&mut ReviewCard, Option<&PracticeSession>) -> Result<bool, String>,
        event: impl FnOnce(&ReviewCard) -> Option<LearningEvent>,
    ) -> Result<Checked<ReviewCard>, StoreError> {
        let tx = self.db.begin_write()?;
        let result = {
            let session = match practice {
                Some(p) => tx.open_table(PRACTICE)?.get(p)?.map(|v| serde_json::from_str::<PracticeSession>(v.value())).transpose()?,
                None => None,
            };
            let mut t = tx.open_table(REVIEWS)?;
            let current = t.get(id)?.map(|v| serde_json::from_str::<ReviewCard>(v.value())).transpose()?;
            match current {
                None => Checked::Unchanged,
                Some(mut card) => match f(&mut card, session.as_ref()) {
                    Ok(true) => {
                        card.revision += 1;
                        t.insert(id, serde_json::to_string(&card)?.as_str())?;
                        if let Some(e) = event(&card) {
                            insert_events(&mut tx.open_table(EVENTS)?, std::slice::from_ref(&e))?;
                        }
                        Checked::Written(card)
                    }
                    Ok(false) => Checked::Unchanged,
                    Err(reason) => Checked::Rejected(reason),
                },
            }
        };
        tx.commit()?;
        Ok(result)
    }

    pub fn load_review(&self, id: &str) -> Result<Option<ReviewCard>, StoreError> { self.get(REVIEWS, id) }

    /// 与 `update_review_if` 相同；写回时在同一个写事务中用 `g` 更新编号为 `event_id` 的学习事件（存在时）。
    pub fn update_review_and_event(
        &self, id: &str, f: impl FnOnce(&mut ReviewCard) -> bool, event_id: &str, g: impl FnOnce(&ReviewCard, &mut LearningEvent),
    ) -> Result<Option<ReviewCard>, StoreError> {
        let tx = self.db.begin_write()?;
        let updated = {
            let mut t = tx.open_table(REVIEWS)?;
            let current = t.get(id)?.map(|v| serde_json::from_str::<ReviewCard>(v.value())).transpose()?;
            match current {
                Some(mut card) => {
                    if !f(&mut card) {
                        return Ok(None);
                    }
                    card.revision += 1;
                    t.insert(id, serde_json::to_string(&card)?.as_str())?;
                    let mut events = tx.open_table(EVENTS)?;
                    let event = events.get(event_id)?.map(|v| serde_json::from_str::<LearningEvent>(v.value())).transpose()?;
                    if let Some(mut e) = event {
                        g(&card, &mut e);
                        events.insert(event_id, serde_json::to_string(&e)?.as_str())?;
                    }
                    Some(card)
                }
                None => None,
            }
        };
        tx.commit()?;
        Ok(updated)
    }

    /// 写入学习事件；同一编号已有时不覆盖。返回实际写入的条数。
    pub fn insert_events(&self, events: &[LearningEvent]) -> Result<usize, StoreError> {
        let tx = self.db.begin_write()?;
        let n = insert_events(&mut tx.open_table(EVENTS)?, events)?;
        tx.commit()?;
        Ok(n)
    }

    pub fn list_events(&self) -> Result<Vec<LearningEvent>, StoreError> { self.list(EVENTS, "学习事件") }

    pub fn load_event(&self, id: &str) -> Result<Option<LearningEvent>, StoreError> { self.get(EVENTS, id) }

    /// 是否已经从早期的记录补上学习事件。
    pub fn events_backfilled(&self) -> Result<bool, StoreError> {
        let tx = self.db.begin_read()?;
        Ok(tx.open_table(META)?.get(EVENTS_BACKFILLED)?.is_some())
    }

    /// 补上早期记录的学习事件并记下已经补过，在同一个写事务中完成。返回写入的条数。
    pub fn backfill_events(&self, events: &[LearningEvent]) -> Result<usize, StoreError> {
        let tx = self.db.begin_write()?;
        let n = insert_events(&mut tx.open_table(EVENTS)?, events)?;
        tx.open_table(META)?.insert(EVENTS_BACKFILLED, "1")?;
        tx.commit()?;
        Ok(n)
    }

    pub fn list_reviews(&self) -> Result<Vec<ReviewCard>, StoreError> { self.list(REVIEWS, "复习卡片") }

    /// 开始一组练习：在同一写事务中把其他还没有结束的练习记为结束、写入这一组，
    /// 再按开始时间只保留最近 `keep` 组（含这一组）。
    pub fn start_practice(&self, session: &PracticeSession, keep: usize) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(PRACTICE)?;
            let mut earlier: Vec<PracticeSession> = Vec::new();
            for item in t.iter()? {
                let (_, v) = item?;
                match serde_json::from_str::<PracticeSession>(v.value()) {
                    Ok(s) => earlier.push(s),
                    Err(e) => tracing::warn!("跳过无法解析的练习批次：{e}"),
                }
            }
            for s in earlier.iter_mut().filter(|s| s.finished_at.is_none()) {
                s.finished_at = Some(session.started_at);
                s.updated_at = session.started_at;
                s.revision += 1;
                t.insert(s.id.as_str(), serde_json::to_string(&*s)?.as_str())?;
            }
            t.insert(session.id.as_str(), serde_json::to_string(session)?.as_str())?;
            earlier.sort_by_key(|s| std::cmp::Reverse(s.started_at));
            for old in earlier.iter().skip(keep.saturating_sub(1)) {
                t.remove(old.id.as_str())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_practice(&self, id: &str) -> Result<Option<PracticeSession>, StoreError> { self.get(PRACTICE, id) }

    pub fn list_practice(&self) -> Result<Vec<PracticeSession>, StoreError> { self.list(PRACTICE, "练习批次") }

    /// 修改一组练习：与 `update_review_if` 相同，同一写事务中检查与写回，写回时修改次数加一。
    pub fn update_practice(&self, id: &str, f: impl FnOnce(&mut PracticeSession) -> bool) -> Result<Option<PracticeSession>, StoreError> {
        self.update_if(PRACTICE, id, |s: &mut PracticeSession| {
            let write = f(s);
            if write {
                s.revision += 1;
            }
            write
        })
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

/// 写入学习事件，同一编号已有时不覆盖；返回写入的条数。
fn insert_events(t: &mut redb::Table<&str, &str>, events: &[LearningEvent]) -> Result<usize, StoreError> {
    let mut n = 0;
    for e in events {
        if t.get(e.id.as_str())?.is_none() {
            t.insert(e.id.as_str(), serde_json::to_string(e)?.as_str())?;
            n += 1;
        }
    }
    Ok(n)
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
