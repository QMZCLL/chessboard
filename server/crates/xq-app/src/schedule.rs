//! 复习的安排：状态（待复习、需要再练、已理解）之外，记录下一次复习的时间。
//!
//! 标记状态时按最近一次复习安排间隔（档位见 `ReviewConfig::intervals_days`，默认 1、3、7、14、30 天）：
//! - 标为需要再练：回到第一档；
//! - 标为已理解，但这次直接看了答案或没有独立选对（复现棋谱与棋谱不同，局面决策中这次比较不支持这个选择）：
//!   档位不升，用第一档，近期再看；
//! - 局面决策无法评价（分析不完整、没有得到结果）：档位不升也不降；比较还没有结果时先这样安排，
//!   结果写回时再按结果重新安排（见 `decision::finish_decision`）；
//! - 独立选对后标为已理解：升一档，间隔逐步拉长。
//!
//! 一次安排绑定所依据的那次复习（`ScheduleBase`）：同一次复习重复标记，都从安排前的档位与第一次安排的时间重新计算，
//! 不会把同一次复习当作多次成功；改了评价时按新的评价重新计算；有了新的复习之后才能继续升档。
//!
//! 间隔是可以调整的初始设计，不宣称对所有人都最合适。早期的卡片没有安排：已理解的按上次复习后第三档计，
//! 其余按加入时到期。

use serde::Deserialize;
use serde::Serialize;

use crate::decision::DecisionState;
use crate::decision::DecisionSupport;
use crate::decision::PracticeGoal;
use crate::review::ReviewAttempt;
use crate::review::ReviewCard;
use crate::review::ReviewStatus;

pub const DAY_MS: i64 = 86_400_000;

/// 最近一次安排的起点：所依据的复习（按时间）、安排前的档位与第一次安排的时间。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleBase {
    /// 所依据的复习；还没有复习过时为空。
    pub attempt_at: Option<i64>,
    pub stage: u32,
    pub at: i64,
}

/// 一次复习的结果，用于安排间隔。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 没有走子，直接看了答案（或还没有复习过）。
    Revealed,
    /// 走了，但没有独立选对。
    Missed,
    /// 独立选对：复现棋谱与棋谱一致；局面决策中这次比较支持这个选择。
    Independent,
    /// 局面决策的比较还没有结果。
    Pending,
    /// 局面决策无法评价：分析不完整或没有得到结果。
    Ungraded,
}

pub fn outcome_of(attempt: Option<&ReviewAttempt>) -> Outcome {
    let Some(a) = attempt.filter(|a| a.choice.is_some()) else { return Outcome::Revealed };
    match a.goal {
        PracticeGoal::Recall if a.matched => Outcome::Independent,
        PracticeGoal::Recall => Outcome::Missed,
        // 局面决策按这次比较是否支持这个选择，与是否与棋谱一致分开；不能把没有结果折算成选对或选错
        PracticeGoal::Decision => match &a.decision {
            Some(d) if d.state == DecisionState::Pending => Outcome::Pending,
            Some(d) => match d.support_for(a.choice.as_ref().map(|c| c.mv)) {
                DecisionSupport::Supported => Outcome::Independent,
                DecisionSupport::NotSupported => Outcome::Missed,
                DecisionSupport::Unknown | DecisionSupport::Legacy => Outcome::Ungraded,
            },
            None => Outcome::Ungraded,
        },
    }
}

/// 标记状态之后的档位与间隔（天）。`intervals` 至少有一项（配置保证）。
pub fn next_review(stage: u32, status: ReviewStatus, outcome: Outcome, intervals: &[u32]) -> (u32, u32) {
    let days = |stage: u32| intervals[(stage as usize).min(intervals.len() - 1)];
    match (status, outcome) {
        (ReviewStatus::Understood, Outcome::Independent) => {
            let next = (stage + 1).min(intervals.len() as u32 - 1);
            (next, days(next))
        }
        // 无法评价或还没有结果：不升档也不降档
        (ReviewStatus::Understood, Outcome::Pending | Outcome::Ungraded) => (stage, days(stage)),
        (ReviewStatus::Understood, Outcome::Revealed | Outcome::Missed) => (stage, days(0)),
        (ReviewStatus::Again, _) => (0, days(0)),
        // 待复习：立即到期
        (ReviewStatus::Due, _) => (0, 0),
    }
}

/// 标记状态时安排下一次复习（见模块说明）：同一次复习重复标记，从同一个起点重新计算。
pub fn apply(card: &mut ReviewCard, status: ReviewStatus, now: i64, intervals: &[u32]) {
    let last = card.attempts.last();
    let key = last.map(|a| a.at);
    let base = match card.schedule_base {
        Some(b) if b.attempt_at == key => b,
        _ => ScheduleBase { attempt_at: key, stage: card.stage, at: now },
    };
    let (stage, days) = next_review(base.stage, status, outcome_of(last), intervals);
    card.stage = stage;
    card.due_at = Some(base.at + i64::from(days) * DAY_MS);
    card.schedule_base = Some(base);
}

/// 卡片下一次复习的时间；没有安排时按早期卡片的规则推算（不写回）。
pub fn due_at(card: &ReviewCard, intervals: &[u32]) -> i64 {
    card.due_at.unwrap_or_else(|| match card.status {
        ReviewStatus::Understood => {
            card.reviewed_at.unwrap_or(card.created_at) + i64::from(intervals[2.min(intervals.len() - 1)]) * DAY_MS
        }
        _ => card.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::DecisionFeedback;
    use crate::decision::DecisionVerdict;
    use crate::decision::DecisionState;
    use crate::review::ReviewMove;

    const DAYS: [u32; 5] = [1, 3, 7, 14, 30];

    fn attempt(choice: bool, matched: bool, goal: PracticeGoal, verdict: Option<DecisionVerdict>) -> ReviewAttempt {
        ReviewAttempt {
            at: 1,
            choice: choice.then(|| ReviewMove { mv: xq_core::Move::parse_iccs("h9g7").unwrap(), notation: "马8进7".into() }),
            matched,
            goal,
            intent: None,
            reflection: None,
            decision: verdict.map(|v| DecisionFeedback {
                state: DecisionState::Ready,
                verdict: Some(v),
                // 按早期记录的规则从结论推算
                support: crate::decision::DecisionSupport::Legacy,
                reference: None,
                points: vec![],
                conditions: None,
                error: None,
            }),
        }
    }

    #[test]
    fn independent_answers_lengthen_the_interval_and_others_come_back_soon() {
        use Outcome::*;
        use ReviewStatus::*;
        // 独立选对后标为已理解：逐档拉长，到最后一档为止
        assert_eq!(next_review(0, Understood, Independent, &DAYS), (1, 3));
        assert_eq!(next_review(1, Understood, Independent, &DAYS), (2, 7));
        assert_eq!(next_review(4, Understood, Independent, &DAYS), (4, 30));
        // 直接看答案或没有独立选对：档位不升，明天再看
        assert_eq!(next_review(2, Understood, Revealed, &DAYS), (2, 1));
        assert_eq!(next_review(2, Understood, Missed, &DAYS), (2, 1));
        // 需要再练：回到第一档
        assert_eq!(next_review(3, Again, Independent, &DAYS), (0, 1));
        assert_eq!(next_review(3, Due, Independent, &DAYS), (0, 0));
        // 只有一档时不越界
        assert_eq!(next_review(0, Understood, Independent, &[2]), (0, 2));
    }

    #[test]
    fn outcomes_follow_the_practice_goal() {
        use DecisionVerdict::*;
        use PracticeGoal::*;
        assert_eq!(outcome_of(None), Outcome::Revealed);
        assert_eq!(outcome_of(Some(&attempt(false, false, Recall, None))), Outcome::Revealed);
        assert_eq!(outcome_of(Some(&attempt(true, true, Recall, None))), Outcome::Independent);
        assert_eq!(outcome_of(Some(&attempt(true, false, Recall, None))), Outcome::Missed);
        assert_eq!(outcome_of(Some(&attempt(true, false, Decision, Some(Reasonable)))), Outcome::Independent);
        assert_eq!(outcome_of(Some(&attempt(true, false, Decision, Some(Noteworthy)))), Outcome::Missed);
        // 无法判断：不算选对，也不算选错
        assert_eq!(outcome_of(Some(&attempt(true, false, Decision, Some(Unclear)))), Outcome::Ungraded);
        // 与棋谱一致，但这次比较不支持（棋谱着法明显不如引擎参考）：不算独立选对
        let mut known_bad = attempt(true, true, Decision, Some(SameAsRecord));
        known_bad.decision.as_mut().unwrap().support = crate::decision::DecisionSupport::NotSupported;
        assert_eq!(outcome_of(Some(&known_bad)), Outcome::Missed);
        assert_eq!(outcome_of(Some(&attempt(true, true, Decision, Some(Unclear)))), Outcome::Ungraded);
    }

    fn card_with(attempts: Vec<ReviewAttempt>) -> ReviewCard {
        let mut c: ReviewCard = serde_json::from_value(serde_json::json!({
            "id": "s/1", "study_id": "s", "node_id": 1, "study_name": "测试", "created_at": 1, "updated_at": 1,
            "status": "due", "fen": "4k4/9/9/9/9/9/9/9/9/4K4 w", "side": "red", "orientation": "red_bottom",
            "turn": 1, "history": [], "history_complete": true, "answer": { "mv": "h9g7", "notation": "马8进7" },
            "thinking": null, "basis": null, "note": "", "attempts": [], "reviewed_at": null
        }))
        .unwrap();
        c.attempts = attempts;
        c
    }

    #[test]
    fn marking_the_same_attempt_again_does_not_advance_twice() {
        let mut card = card_with(vec![attempt(true, true, PracticeGoal::Recall, None)]);
        apply(&mut card, ReviewStatus::Understood, 1_000, &DAYS);
        let first = (card.stage, card.due_at);
        assert_eq!(first, (1, Some(1_000 + 3 * DAY_MS)));
        // 同一次复习再标一次：档位与到期时间都不变
        apply(&mut card, ReviewStatus::Understood, 9_000, &DAYS);
        assert_eq!((card.stage, card.due_at), first);
        // 改成需要再练再改回来：都按同一个起点计算
        apply(&mut card, ReviewStatus::Again, 9_500, &DAYS);
        assert_eq!((card.stage, card.due_at), (0, Some(1_000 + DAY_MS)));
        apply(&mut card, ReviewStatus::Understood, 9_800, &DAYS);
        assert_eq!((card.stage, card.due_at), first);
        // 有了新的复习之后才继续升档
        let mut next = attempt(true, true, PracticeGoal::Recall, None);
        next.at = 2;
        card.attempts.push(next);
        apply(&mut card, ReviewStatus::Understood, 20_000, &DAYS);
        assert_eq!((card.stage, card.due_at), (2, Some(20_000 + 7 * DAY_MS)));
    }

    #[test]
    fn a_pending_or_ungraded_decision_neither_advances_nor_counts_as_missed() {
        use DecisionVerdict::*;
        let pending = || {
            let mut a = attempt(true, false, PracticeGoal::Decision, Some(Reasonable));
            a.decision.as_mut().unwrap().state = DecisionState::Pending;
            a
        };
        assert_eq!(outcome_of(Some(&pending())), Outcome::Pending);
        let mut unclear = attempt(true, false, PracticeGoal::Decision, Some(Unclear));
        assert_eq!(outcome_of(Some(&unclear)), Outcome::Ungraded);
        unclear.decision.as_mut().unwrap().state = DecisionState::Failed;
        assert_eq!(outcome_of(Some(&unclear)), Outcome::Ungraded);
        // 比较还没有结果时标为已理解：保持档位
        let mut card = card_with(vec![pending()]);
        card.stage = 2;
        apply(&mut card, ReviewStatus::Understood, 1_000, &DAYS);
        assert_eq!((card.stage, card.due_at), (2, Some(1_000 + 7 * DAY_MS)));
        // 结果回来、支持这个选择：按同一个起点重新安排，升一档
        let d = card.attempts[0].decision.as_mut().unwrap();
        d.state = DecisionState::Ready;
        d.support = DecisionSupport::Supported;
        apply(&mut card, ReviewStatus::Understood, 5_000, &DAYS);
        assert_eq!((card.stage, card.due_at), (3, Some(1_000 + 14 * DAY_MS)));
    }
}
