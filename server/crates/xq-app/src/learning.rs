//! 学习事件：每次作答记一条，是能力画像的依据。
//!
//! - 来源：复习（含练习批次中的作答）与先想后讲。复习的事件与卡片中的作答一一对应（编号为“卡片/作答时间”），
//!   在写入作答的同一个写事务中写入；局面决策的比较结果晚到时，在写回比较结果的同一个写事务中更新结果。
//! - 卡片只保留最近 20 次作答，事件保留完整的历史；删除卡片或会话时事件保留。
//! - 第一次启动（以及导入学习档案）时，从已有的作答与先想后讲的练习记录补上缺少的事件；
//!   早期的作答没有记录耗时与信心，补上的事件中也没有。
//! - 结果的分法与安排复习间隔相同（见 `schedule::outcome_of`）：用了提示之后选对另记为“看了提示后选对”，
//!   不算独立选对。

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;
use xq_core::explain::stability::StabilityLevel;
use xq_core::study::Node;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::decision::Intent;
use crate::decision::PracticeGoal;
use crate::review::ReviewAttempt;
use crate::review::ReviewCard;
use crate::schedule;
use crate::schedule::Outcome;
use crate::topics;
use crate::topics::Issue;
use crate::topics::Theme;

/// 作答的来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EventSource {
    /// 复习（含练习批次中的作答）。
    Review,
    /// 先想后讲。
    Think,
}

/// 一次作答的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EventOutcome {
    /// 没有用提示，独立选对（复现棋谱与棋谱一致；局面决策中比较支持这个选择）。
    Independent,
    /// 用了提示之后选对。
    Assisted,
    /// 走了，但没有选对。
    Missed,
    /// 没有走子，直接看了答案。
    Revealed,
    /// 局面决策的比较还没有结果。
    Pending,
    /// 局面决策无法评价（分析不完整、没有得到结果或排名仍在变化）。
    Ungraded,
}

impl From<Outcome> for EventOutcome {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Independent => EventOutcome::Independent,
            Outcome::Assisted => EventOutcome::Assisted,
            Outcome::Missed => EventOutcome::Missed,
            Outcome::Revealed => EventOutcome::Revealed,
            Outcome::Pending => EventOutcome::Pending,
            Outcome::Ungraded => EventOutcome::Ungraded,
        }
    }
}

/// 作答时自己选择的信心（可以不选）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Confidence {
    /// 基本靠猜。
    Guess,
    /// 有些把握。
    Unsure,
    /// 很有把握。
    Sure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LearningEvent {
    /// 复习为“卡片/作答时间”，先想后讲为“会话/节点/think/时间”。
    pub id: String,
    pub at: i64,
    pub source: EventSource,
    /// 复习的卡片；先想后讲为空。
    pub card: Option<String>,
    pub study_id: String,
    pub node_id: NodeId,
    /// 作答的局面（落子前，含行棋方）。
    pub fen: String,
    pub goal: PracticeGoal,
    pub outcome: EventOutcome,
    /// 棋谱中这一着的规则主题（与卡片的主题同一套判断）。
    pub themes: Vec<Theme>,
    /// 作答时已经确认的“我当时的问题”。
    pub issues: Vec<Issue>,
    /// 用了几级提示。
    pub hints: u8,
    pub intent: Option<Intent>,
    /// 从显示局面到作答的时间；只记录，不评分。补上的事件为空。
    pub elapsed_ms: Option<u64>,
    pub confidence: Option<Confidence>,
    /// 局面决策比较搜索的稳定性。
    pub stability: Option<StabilityLevel>,
    /// 所属的练习批次。
    pub practice: Option<String>,
}

pub fn review_event_id(card: &str, at: i64) -> String { format!("{card}/{at:013}") }

pub fn think_event_id(study: &str, node: NodeId, at: i64) -> String { format!("{study}/{node:010}/think/{at:013}") }

/// 一次复习的事件（与卡片中的作答一一对应）。
pub fn from_attempt(card: &ReviewCard, a: &ReviewAttempt) -> LearningEvent {
    LearningEvent {
        id: review_event_id(&card.id, a.at),
        at: a.at,
        source: EventSource::Review,
        card: Some(card.id.clone()),
        study_id: card.study_id.clone(),
        node_id: card.node_id,
        fen: card.fen.clone(),
        goal: a.goal,
        outcome: schedule::outcome_of(Some(a)).into(),
        themes: topics::card_themes(&card.fen, card.answer.mv).into_iter().map(|t| t.theme).collect(),
        issues: card.issues.clone(),
        hints: a.hints,
        intent: a.intent,
        elapsed_ms: a.elapsed_ms,
        confidence: a.confidence,
        stability: a.decision.as_ref().and_then(|d| d.stability),
        practice: a.practice.clone(),
    }
}

/// 一张卡片中每次作答的事件。
pub fn from_card(card: &ReviewCard) -> Vec<LearningEvent> { card.attempts.iter().map(|a| from_attempt(card, a)).collect() }

/// 先想后讲中这一着最近一次练习的事件（节点只保存最近一次）：与棋谱一致算选对，用了提示另记。
pub fn from_practice(study: &Study, node: &Node) -> Option<LearningEvent> {
    let p = node.practice.as_ref()?;
    let mv = node.mv?;
    let before = study.node(node.parent?).ok()?;
    let outcome = if p.revealed_directly || p.first_choice.is_none() {
        EventOutcome::Revealed
    } else if !p.matched {
        EventOutcome::Missed
    } else if p.hints_used > 0 {
        EventOutcome::Assisted
    } else {
        EventOutcome::Independent
    };
    Some(LearningEvent {
        id: think_event_id(&study.id, node.id, p.updated_at),
        at: p.updated_at,
        source: EventSource::Think,
        card: None,
        study_id: study.id.clone(),
        node_id: node.id,
        fen: before.fen.clone(),
        goal: PracticeGoal::Recall,
        outcome,
        themes: topics::card_themes(&before.fen, mv).into_iter().map(|t| t.theme).collect(),
        issues: Vec::new(),
        hints: p.hints_used,
        intent: None,
        elapsed_ms: None,
        confidence: None,
        stability: None,
        practice: None,
    })
}

/// 一个会话中先想后讲的事件。
pub fn from_study(study: &Study) -> Vec<LearningEvent> { study.nodes.values().filter_map(|n| from_practice(study, n)).collect() }

/// 导入的事件是否成立（编号、局面与提示级数），不成立时返回原因。
pub(crate) fn check_event(e: &LearningEvent) -> Result<(), String> {
    if e.id.is_empty() || e.id.len() > 200 {
        return Err("事件编号无效".into());
    }
    xq_core::fen::parse_position(&e.fen).map_err(|err| format!("事件的局面无法读取：{err}"))?;
    if e.hints > 3 {
        return Err("事件的提示级数超过 3".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use xq_core::Move;
    use xq_core::Position;
    use xq_core::study::MoveSource;
    use xq_core::study::NewStudy;
    use xq_core::study::Orientation;
    use xq_core::study::ROOT;
    use xq_core::study::RuleSet;
    use xq_core::study::StudySource;

    use super::*;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    #[test]
    fn think_practice_becomes_an_event_with_hints_counted_separately() {
        let mut s = Study::new(NewStudy {
            id: "s".into(),
            name: "测试".into(),
            source: StudySource::Standard,
            start: Position::startpos(),
            counters: None,
            history_complete: true,
            orientation: Orientation::RedBottom,
            user_side: xq_core::Color::Red,
            rules: RuleSet::default(),
            now: 0,
        })
        .unwrap();
        let n = s.apply_move(ROOT, mv("h2e2"), MoveSource::User, 1).unwrap().node;
        assert!(from_study(&s).is_empty(), "没有练习过的着法没有事件");
        let outcome = |s: &Study| from_practice(s, s.node(n).unwrap()).unwrap().outcome;
        s.record_practice(n, 0, false, Some(mv("h2e2")), None, 5).unwrap();
        assert_eq!(outcome(&s), EventOutcome::Independent);
        s.record_practice(n, 2, false, Some(mv("h2e2")), None, 6).unwrap();
        assert_eq!(outcome(&s), EventOutcome::Assisted, "用了提示之后选对不算独立选对");
        s.record_practice(n, 0, false, Some(mv("h0g2")), None, 7).unwrap();
        assert_eq!(outcome(&s), EventOutcome::Missed);
        s.record_practice(n, 0, true, None, None, 8).unwrap();
        let e = from_practice(&s, s.node(n).unwrap()).unwrap();
        assert_eq!((e.outcome, e.id.as_str(), e.fen.as_str()), (EventOutcome::Revealed, "s/0000000001/think/0000000000008", xq_core::fen::START_FEN));
    }
}
