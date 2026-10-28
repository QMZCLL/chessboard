//! 复习卡片：把学过的一着连同当时的思考保存下来。复习时先只给出局面，让用户重新选择，
//! 再对照原来的思考、棋谱中的这一着与程序讲解。
//!
//! 卡片在加入复习时保存快照：落子前的局面与此前的着法、先想后讲时的第一次选择与看过的提示、
//! 棋谱中的这一着、程序讲解的要点与依据（连同分析条件与讲解规则版本）、用户自己的理解或疑问。
//! 原会话之后被修改或删除，卡片仍按快照复习；只有“打开原局面”需要原会话中的这一着仍在。
//! 第一版只有三种状态（待复习、需要再练、已理解），不做间隔调度。
//!
//! 每次复习有一个练习目标：复现棋谱只看是否与棋谱相同；局面决策把这次的选择与棋谱着法、引擎参考
//! 放在同一次搜索中比较（见 `decision`）。落子前可以选择意图，揭示后与这一步实际的规则事实对照。

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use xq_core::Color;
use xq_core::Move;
use xq_core::explain::ExplainContext;
use xq_core::explain::ExplainLevel;
use xq_core::explain::Verdict;
use xq_core::explain::hints::UsedHints;
use xq_core::fen;
use xq_core::notation;
use xq_core::rules;
use xq_core::study::NodeId;
use xq_core::study::Orientation;
use xq_core::study::Study;

use crate::decision;
use crate::decision::DecisionFeedback;
use crate::decision::DecisionState;
use crate::decision::Intent;
use crate::decision::PracticeGoal;
use crate::decision::Reflection;
use crate::records::Budget;
use crate::schedule;
use crate::topics;
use crate::topics::Issue;
use crate::topics::ThemeTag;
use crate::service::ApiError;
use crate::service::ApiResult;
use crate::service::Inner;
use crate::service::now_ms;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    /// 待复习：刚加入，或还没有复习过。
    Due,
    /// 需要再练。
    Again,
    /// 已理解。
    Understood,
}

impl ReviewStatus {
    /// 列表中的顺序：需要再练、待复习在前。
    fn order(self) -> u8 {
        match self {
            ReviewStatus::Again => 0,
            ReviewStatus::Due => 1,
            ReviewStatus::Understood => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewMove {
    pub mv: Move,
    pub notation: String,
}

/// 加入复习时的思考记录（来自这一着最近一次先想后讲的练习）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewThinking {
    pub first_choice: Option<ReviewMove>,
    pub hints_used: u8,
    pub revealed_directly: bool,
    pub matched: bool,
    /// 当时看过的提示原文；早期的练习记录没有保存原文。
    pub hints: Option<UsedHints>,
    pub practiced_at: i64,
}

/// 加入复习时这一着的程序讲解：要点、依据与分析版本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewBasis {
    pub level: ExplainLevel,
    pub summary: String,
    pub verdict: Option<Verdict>,
    pub need: Vec<String>,
    pub basis: Vec<String>,
    pub replies: Vec<String>,
    pub lesson: String,
    pub warnings: Vec<String>,
    /// 引擎分析条件（引擎、预算、深度等）。
    pub conditions: Option<String>,
    pub budget: Option<Budget>,
    /// 讲解规则的版本。
    pub generator_version: u32,
    pub explained_at: i64,
}

/// 一次复习：重新选择的着法（直接看答案时为空）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAttempt {
    /// 复习的时间；同一张卡片内唯一，局面决策的比较结果按它写回。
    pub at: i64,
    pub choice: Option<ReviewMove>,
    pub matched: bool,
    /// 练习目标；早期的记录没有这一项，为复现棋谱。
    #[serde(default)]
    pub goal: PracticeGoal,
    /// 落子前选择的意图。
    #[serde(default)]
    pub intent: Option<Intent>,
    /// 这一步实际改变了什么，以及与意图的对照；直接看答案时为空。
    #[serde(default)]
    pub reflection: Option<Reflection>,
    /// 局面决策中与棋谱着法、引擎参考的比较；复现棋谱或直接看答案时为空。
    #[serde(default)]
    pub decision: Option<DecisionFeedback>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCard {
    /// 与讲解记录相同的“会话/节点”：同一着只有一张卡片。
    pub id: String,
    pub study_id: String,
    pub node_id: NodeId,
    /// 加入时的会话名称。
    pub study_name: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// 修改次数：每次修改加一（见 `Store::update_review`）。界面据此判断同一张卡片的两份数据哪份较新；
    /// 更新时间分不出同一毫秒内的两次修改，补上讲解也不改更新时间。早期的卡片没有这一项，从 0 算起。
    #[serde(default)]
    pub revision: u64,
    pub status: ReviewStatus,
    /// 落子前局面（含行棋方），复习时先显示它。
    pub fen: String,
    /// 这一着的走棋方。
    pub side: Color,
    pub orientation: Orientation,
    /// 这一着在第几回合。
    pub turn: u32,
    /// 本段棋谱起点到这个局面的着法。
    pub history: Vec<ReviewMove>,
    /// 起点之前的历史是否完整（残局、截图起点为不完整）。
    pub history_complete: bool,
    /// 棋谱中的这一着，作为对照。
    pub answer: ReviewMove,
    pub thinking: Option<ReviewThinking>,
    pub basis: Option<ReviewBasis>,
    /// 用户自己的理解或疑问；加入时从这一着的笔记带入。
    pub note: String,
    pub attempts: Vec<ReviewAttempt>,
    pub reviewed_at: Option<i64>,
    /// 下一次复习的时间（见 `schedule`）；早期的卡片没有这一项，返回给界面时按规则推算。
    #[serde(default)]
    pub due_at: Option<i64>,
    /// 复习间隔的档位：独立选对后标为已理解时升一档。
    #[serde(default)]
    pub stage: u32,
    /// 用户自己确认的问题（见 `topics`）。
    #[serde(default)]
    pub issues: Vec<Issue>,
}

/// 保留的复习次数。
pub(crate) const MAX_ATTEMPTS: usize = 20;

/// 上次退出时还在进行的局面决策比较已经不会完成：标为没有得到结果，返回处理的卡片数。
pub(crate) fn reset_interrupted_decisions(store: &crate::storage::Store) -> Result<usize, crate::storage::StoreError> {
    let mut count = 0;
    for card in store.list_reviews()? {
        let pending = |a: &ReviewAttempt| a.decision.as_ref().is_some_and(|d| d.state == DecisionState::Pending);
        if !card.attempts.iter().any(pending) {
            continue;
        }
        store.update_review(&card.id, |c| {
            for d in c.attempts.iter_mut().filter_map(|a| a.decision.as_mut()).filter(|d| d.state == DecisionState::Pending) {
                d.state = DecisionState::Failed;
                d.error = Some("上次比较在应用退出前没有完成".into());
            }
        })?;
        count += 1;
    }
    Ok(count)
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewItem {
    pub card: ReviewCard,
    /// 原会话中的这一着仍在，可以打开原局面。
    pub source_available: bool,
    /// 原会话当前的名称（可能已改名）；原会话不在时为加入时的名称。
    pub study_name: String,
    /// 程序根据这一着的规则事实建议的主题（附依据）。
    pub themes: Vec<ThemeTag>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewDetail {
    #[serde(flatten)]
    pub item: ReviewItem,
    /// 这个局面的合法着法，用于在棋盘上重新选择。
    pub legal_moves: Vec<Move>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReviewRef {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReviewAttemptRequest {
    pub id: String,
    /// 重新选择的着法；为空表示没有走子，直接查看原来的思考与讲解。
    pub choice: Option<Move>,
    #[serde(default)]
    pub goal: PracticeGoal,
    #[serde(default)]
    pub intent: Option<Intent>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReviewUpdateRequest {
    pub id: String,
    pub status: Option<ReviewStatus>,
    pub note: Option<String>,
    /// 用户确认的问题（整组替换）。
    #[serde(default)]
    pub issues: Option<Vec<Issue>>,
}

pub fn card_id(study_id: &str, node_id: NodeId) -> String { format!("{study_id}/{node_id:010}") }

/// 回合数：根局面黑先时第一着就是第一回合的黑方。
pub(crate) fn turn_of(ply: u32, root_side: Color) -> u32 { (ply.saturating_sub(1) + u32::from(root_side == Color::Black)) / 2 + 1 }

fn source_available(study: Option<&Study>, card: &ReviewCard) -> bool {
    study.and_then(|s| s.nodes.get(&card.node_id)).is_some_and(|n| n.mv == Some(card.answer.mv))
}

impl Inner {
    /// 这一着的程序讲解快照；还没有完成的讲解不保存。
    fn review_basis(&self, study_id: &str, node_id: NodeId) -> Option<ReviewBasis> {
        let record = self.store.load_explanation(study_id, node_id).ok().flatten()?;
        let e = record.explanation.filter(|_| !record.pending)?;
        let texts = |claims: &[xq_core::explain::Claim]| claims.iter().map(|c| c.text.clone()).collect();
        Some(ReviewBasis {
            level: e.level,
            summary: e.summary.clone(),
            verdict: e.verdict.clone(),
            need: texts(&e.need),
            basis: texts(&e.basis),
            replies: texts(&e.replies),
            lesson: e.lesson.clone(),
            warnings: e.warnings.clone(),
            conditions: e.conditions.clone(),
            budget: record.budget,
            generator_version: e.generator_version,
            explained_at: record.updated_at,
        })
    }

    fn build_card(&self, study: &Study, node_id: NodeId) -> ApiResult<ReviewCard> {
        let node = study.node(node_id)?;
        let (Some(answer), Some(parent)) = (node.mv, node.parent) else {
            return Err("起始局面和缺口没有着法，不能加入复习".into());
        };
        let before = study.node(parent)?;
        let pos = study.position(parent)?;
        let name = |mv: Move| notation::move_to_chinese(&pos, mv).unwrap_or_else(|_| mv.iccs());
        // 本段起点（根或缺口）之后、这个局面之前的着法
        let start = study.segment_start(parent)?;
        let path = study.path(parent)?;
        let from = path.iter().position(|n| *n == start).unwrap_or(0);
        let history = path[from + 1..]
            .iter()
            .filter_map(|id| study.nodes.get(id))
            .filter_map(|n| {
                let mv = n.mv?;
                Some(ReviewMove { mv, notation: n.notation.clone().unwrap_or_else(|| mv.iccs()) })
            })
            .collect();
        let root_side = study.node(xq_core::study::ROOT)?.side_to_move;
        let thinking = node.practice.as_ref().map(|p| ReviewThinking {
            first_choice: p.first_choice.map(|mv| ReviewMove { mv, notation: name(mv) }),
            hints_used: p.hints_used,
            revealed_directly: p.revealed_directly,
            matched: p.matched,
            hints: p.hints.clone(),
            practiced_at: p.updated_at,
        });
        let now = now_ms();
        Ok(ReviewCard {
            id: card_id(&study.id, node_id),
            study_id: study.id.clone(),
            node_id,
            study_name: study.name.clone(),
            created_at: now,
            updated_at: now,
            revision: 0,
            status: ReviewStatus::Due,
            fen: fen::position_fen(&pos),
            side: before.side_to_move,
            orientation: study.orientation,
            turn: turn_of(node.ply, root_side),
            history,
            history_complete: study.history_complete_at(parent)?,
            answer: ReviewMove { mv: answer, notation: node.notation.clone().unwrap_or_else(|| name(answer)) },
            thinking,
            basis: self.review_basis(&study.id, node_id),
            note: node.note.clone(),
            attempts: Vec::new(),
            reviewed_at: None,
            // 加入后立即到期
            due_at: Some(now),
            stage: 0,
            issues: Vec::new(),
        })
    }

    fn load_card(&self, id: &str) -> ApiResult<ReviewCard> {
        self.store.load_review(id)?.ok_or_else(|| ApiError::from("找不到这张复习卡片，可能已经删除"))
    }

    pub(crate) fn review_item(&self, card: ReviewCard) -> ReviewItem {
        let study = self.store.load_study(&card.study_id).ok().flatten();
        self.item_of(card, study.as_ref())
    }

    /// 界面看到的卡片：附上原会话的情况；没有安排的早期卡片按规则推算下一次复习的时间。
    fn item_of(&self, mut card: ReviewCard, study: Option<&Study>) -> ReviewItem {
        let intervals = self.config_snapshot().review.intervals_days;
        card.due_at = Some(schedule::due_at(&card, &intervals));
        // 早期的比较结果没有“是否支持这个选择”，按结论推算后再交给界面
        for a in &mut card.attempts {
            let choice = a.choice.as_ref().map(|c| c.mv);
            if let Some(d) = a.decision.as_mut() {
                d.support = d.support_for(choice);
            }
        }
        let source_available = source_available(study, &card);
        let study_name = study.map(|s| s.name.clone()).unwrap_or_else(|| card.study_name.clone());
        let themes = topics::card_themes(&card.fen, card.answer.mv);
        ReviewItem { card, source_available, study_name, themes }
    }

    /// 把一着加入复习；已经在复习中时返回原来的卡片，不覆盖当时的记录。
    pub(crate) fn add_review(&self, study_id: &str, node_id: NodeId) -> ApiResult<ReviewItem> {
        let id = card_id(study_id, node_id);
        if let Some(card) = self.store.load_review(&id)? {
            return Ok(self.review_item(card));
        }
        let study = self.with_study(study_id, |s| Ok(s.clone()))?;
        let card = self.build_card(&study, node_id)?;
        // 同时加入同一着时只保留先写入的一张
        let stored = self.store.insert_review(&card)?;
        Ok(self.review_item(stored))
    }

    /// 修改卡片（见 `Store::update_review`：读取最新的卡片、修改、写回在同一写事务中完成）。
    fn modify_card(&self, id: &str, f: impl FnOnce(&mut ReviewCard)) -> ApiResult<ReviewCard> {
        self.store.update_review(id, f)?.ok_or_else(|| ApiError::from("找不到这张复习卡片，可能已经删除"))
    }

    /// 全部卡片：先到期的在前；同时到期的，需要再练、待复习在前。
    pub(crate) fn list_reviews(&self) -> ApiResult<Vec<ReviewItem>> {
        let studies: HashMap<String, Study> = self.store.list_studies()?.into_iter().map(|s| (s.id.clone(), s)).collect();
        let mut items: Vec<ReviewItem> = self
            .store
            .list_reviews()?
            .into_iter()
            .map(|c| {
                let study = studies.get(&c.study_id);
                self.item_of(c, study)
            })
            .collect();
        items.sort_by_key(|i| (i.card.due_at, i.card.status.order(), i.card.created_at));
        Ok(items)
    }

    pub(crate) fn review_detail(&self, id: &str) -> ApiResult<ReviewDetail> {
        let mut card = self.load_card(id)?;
        // 加入时讲解还没有完成：原会话中的这一着仍在、讲解已经完成时补上（只补空缺，不改动其他内容）
        if card.basis.is_none()
            && let Ok(Some(study)) = self.store.load_study(&card.study_id)
            && source_available(Some(&study), &card)
            && let Some(basis) = self.review_basis(&card.study_id, card.node_id)
        {
            card = self.modify_card(id, |c| {
                c.basis.get_or_insert(basis);
            })?;
        }
        let pos = fen::parse_position(&card.fen).map_err(|e| ApiError::from(format!("卡片中的局面无法读取：{e}")))?;
        let legal_moves = rules::legal_moves(&pos);
        Ok(ReviewDetail { item: self.review_item(card), legal_moves })
    }

    /// 记一次复习：重新选择的着法是否与棋谱相同，这一步实际改变了什么；局面决策还在后台开始比较，
    /// 完成后写回这次复习并推送 `review_updated`。
    pub(crate) fn review_attempt(&self, req: ReviewAttemptRequest) -> ApiResult<ReviewItem> {
        let id = req.id.as_str();
        // 局面与棋谱着法在卡片建立后不再改变，可以先读出来检查着法
        let card = self.load_card(id)?;
        let pos = fen::parse_position(&card.fen).map_err(|e| ApiError::from(format!("卡片中的局面无法读取：{e}")))?;
        let choice = match req.choice {
            Some(mv) if rules::legal_moves(&pos).contains(&mv) => {
                Some(ReviewMove { mv, notation: notation::move_to_chinese(&pos, mv).unwrap_or_else(|_| mv.iccs()) })
            }
            Some(mv) => return Err(format!("{} 在这个局面中不合规则", mv.iccs()).into()),
            None => None,
        };
        let ctx = ExplainContext { history_complete: card.history_complete, repetition_count: 1 };
        let reflection = choice.as_ref().map(|m| decision::reflect(&pos, m.mv, req.intent, &ctx));
        let deciding = req.goal == PracticeGoal::Decision && choice.is_some();
        let now = now_ms();
        let mut at = now;
        let card = self.modify_card(id, |c| {
            let matched = choice.as_ref().is_some_and(|m| m.mv == c.answer.mv);
            // 同一毫秒内的两次复习也要能区分：比较结果按时间写回
            at = c.attempts.last().map_or(now, |a| now.max(a.at + 1));
            let decision = deciding.then(decision::pending);
            c.attempts.push(ReviewAttempt { at, choice: choice.clone(), matched, goal: req.goal, intent: req.intent, reflection, decision });
            if c.attempts.len() > MAX_ATTEMPTS {
                c.attempts.drain(..c.attempts.len() - MAX_ATTEMPTS);
            }
            c.reviewed_at = Some(now);
            c.updated_at = now;
        })?;
        if let (true, Some(m)) = (deciding, &choice) {
            self.review_decide(&card, at, m.mv);
        }
        Ok(self.review_item(card))
    }

    /// 只改请求中给出的字段（状态、笔记）；其他字段保持最新的值。
    /// 标记状态时，按最近一次复习安排下一次复习的时间（见 `schedule`）。
    pub(crate) fn update_review(&self, req: ReviewUpdateRequest) -> ApiResult<ReviewItem> {
        let intervals = self.config_snapshot().review.intervals_days;
        let now = now_ms();
        let card = self.modify_card(&req.id, |c| {
            if let Some(status) = req.status {
                c.status = status;
                let outcome = schedule::outcome_of(c.attempts.last());
                let (stage, days) = schedule::next_review(c.stage, status, outcome, &intervals);
                c.stage = stage;
                c.due_at = Some(now + i64::from(days) * schedule::DAY_MS);
            }
            if let Some(note) = req.note {
                c.note = note;
            }
            if let Some(mut issues) = req.issues {
                issues.sort_unstable();
                issues.dedup();
                c.issues = issues;
            }
            c.updated_at = now;
        })?;
        Ok(self.review_item(card))
    }

    pub(crate) fn delete_review(&self, id: &str) -> ApiResult<()> {
        self.load_card(id)?;
        Ok(self.store.delete_review(id)?)
    }

    /// 这一着是否在复习中，以及当前状态。
    pub(crate) fn review_status(&self, study_id: &str, node_id: NodeId) -> Option<ReviewStatus> {
        self.store.load_review(&card_id(study_id, node_id)).ok().flatten().map(|c| c.status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_numbers_follow_the_side_that_moved_first() {
        assert_eq!((turn_of(1, Color::Red), turn_of(2, Color::Red), turn_of(3, Color::Red)), (1, 1, 2));
        // 黑先的残局：第一着是第一回合的黑方，第二着是第二回合的红方
        assert_eq!((turn_of(1, Color::Black), turn_of(2, Color::Black), turn_of(3, Color::Black)), (1, 2, 2));
    }
}
