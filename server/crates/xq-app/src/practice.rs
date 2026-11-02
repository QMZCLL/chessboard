//! 练习批次：开始一组练习时选定的卡片与顺序、当前的题目、跳过的题目与练习目标，
//! 退出或重新启动后从原来的位置继续。作答本身记在卡片的复习记录中，并带上批次编号
//! （`ReviewAttempt::practice`）：小结只统计属于这一批次的作答，揭示后的状态也按它恢复。
//!
//! - 卡片顺序在开始时确定，之后不变；练习中删除的卡片由界面说明已跳过，继续其余的题目；
//! - “跳过”是没有作答就离开这一题，与“直接看答案”（一次没有走子的作答）分开记录；
//! - 同一时刻只有一组进行中的练习：开始新的一组时，之前还没有结束的一组记为结束；
//! - 只保留最近的若干组（`KEEP`）。

use serde::Deserialize;
use serde::Serialize;

use crate::decision::PracticeGoal;
use crate::service::ApiError;
use crate::service::ApiResult;
use crate::service::Inner;
use crate::service::now_ms;

/// 保留最近的练习批次数。
const KEEP: usize = 20;

/// 这组练习的来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PracticeSource {
    /// 今天到期的卡片。
    Today,
    /// 需要再练的卡片。
    Again,
    /// 某个专题的卡片（见 `topic`）。
    Topic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeSession {
    pub id: String,
    /// 界面上的名称，例如“今天的练习”“专题：保护关系变化”。
    pub label: String,
    pub source: PracticeSource,
    /// 专题练习的专题（界面中的专题键，例如 `issue:missed_reply`）。
    pub topic: Option<String>,
    pub goal: PracticeGoal,
    /// 开始时选定的卡片，按练习的顺序。
    pub cards: Vec<String>,
    /// 当前的题目（`cards` 中的位置）。
    pub index: usize,
    /// 跳过的卡片：没有作答就离开了这一题。
    pub skipped: Vec<String>,
    pub started_at: i64,
    pub updated_at: i64,
    /// 结束的时间；还在进行时为空。
    pub finished_at: Option<i64>,
    /// 修改次数：界面按它判断同一组练习的两份数据中哪一份较新。
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PracticeStartRequest {
    pub label: String,
    pub source: PracticeSource,
    #[serde(default)]
    pub topic: Option<String>,
    pub goal: PracticeGoal,
    /// 选定的卡片，按练习的顺序（重复的只保留第一次）。
    pub cards: Vec<String>,
}

/// 只改请求中给出的部分。
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PracticeUpdateRequest {
    pub id: String,
    /// 换到这一题（`cards` 中的位置）。
    #[serde(default)]
    pub index: Option<usize>,
    /// 把这张卡片记为跳过。
    #[serde(default)]
    pub skip: Option<String>,
    #[serde(default)]
    pub goal: Option<PracticeGoal>,
    /// 结束这组练习；已经结束时不变。
    #[serde(default)]
    pub finish: bool,
}

impl Inner {
    /// 开始一组练习：卡片都要存在；之前还没有结束的一组记为结束。
    pub(crate) fn start_practice(&self, req: PracticeStartRequest) -> ApiResult<PracticeSession> {
        let mut cards: Vec<String> = Vec::new();
        for id in req.cards {
            if !cards.contains(&id) {
                cards.push(id);
            }
        }
        if cards.is_empty() {
            return Err("这组练习没有卡片".into());
        }
        for id in &cards {
            if self.store.load_review(id)?.is_none() {
                return Err(ApiError::from("找不到这组练习中的卡片，可能已经删除"));
            }
        }
        let now = now_ms();
        let session = PracticeSession {
            id: uuid::Uuid::new_v4().to_string(),
            label: req.label,
            source: req.source,
            topic: req.topic,
            goal: req.goal,
            cards,
            index: 0,
            skipped: Vec::new(),
            started_at: now,
            updated_at: now,
            finished_at: None,
            revision: 0,
        };
        self.store.start_practice(&session, KEEP)?;
        Ok(session)
    }

    /// 还没有结束的一组练习（最多一组）。
    pub(crate) fn current_practice(&self) -> ApiResult<Option<PracticeSession>> {
        Ok(self.store.list_practice()?.into_iter().filter(|s| s.finished_at.is_none()).max_by_key(|s| s.started_at))
    }

    /// 换题、记下跳过、改练习目标或结束。结束之后只能再次结束（不变）。
    pub(crate) fn update_practice(&self, req: PracticeUpdateRequest) -> ApiResult<PracticeSession> {
        let now = now_ms();
        let mut refused: Option<&'static str> = None;
        let updated = self.store.update_practice(&req.id, |s| {
            let changes = req.index.is_some() || req.skip.is_some() || req.goal.is_some();
            if s.finished_at.is_some() && changes {
                refused = Some("这组练习已经结束");
                return false;
            }
            if req.index.is_some_and(|i| i >= s.cards.len()) || req.skip.as_ref().is_some_and(|c| !s.cards.contains(c)) {
                refused = Some("这组练习中没有这一题");
                return false;
            }
            if let Some(i) = req.index {
                s.index = i;
            }
            if let Some(card) = &req.skip
                && !s.skipped.contains(card)
            {
                s.skipped.push(card.clone());
            }
            if let Some(goal) = req.goal {
                s.goal = goal;
            }
            if req.finish && s.finished_at.is_none() {
                s.finished_at = Some(now);
            }
            s.updated_at = now;
            true
        })?;
        match (updated, refused) {
            (Some(s), _) => Ok(s),
            (None, Some(reason)) => Err(reason.into()),
            (None, None) => Err("找不到这组练习".into()),
        }
    }

    /// 作答要记入的练习：这组练习存在，并且包含这张卡片。
    pub(crate) fn check_practice(&self, practice: &str, card: &str) -> ApiResult<()> {
        match self.store.load_practice(practice)? {
            Some(s) if s.cards.iter().any(|c| c == card) => Ok(()),
            Some(_) => Err("这组练习中没有这张卡片".into()),
            None => Err("找不到这组练习".into()),
        }
    }
}
