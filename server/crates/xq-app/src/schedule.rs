//! 复习的安排：状态（待复习、需要再练、已理解）之外，记录下一次复习的时间。
//!
//! 标记状态时按最近一次复习安排间隔（档位见 `ReviewConfig::intervals_days`，默认 1、3、7、14、30 天）：
//! - 标为需要再练：回到第一档；
//! - 标为已理解，但这次直接看了答案或没有独立选对（复现棋谱与棋谱不同，局面决策留下值得注意的问题或无法判断）：
//!   档位不升，用第一档，近期再看；
//! - 独立选对后标为已理解：升一档，间隔逐步拉长。
//!
//! 间隔是可以调整的初始设计，不宣称对所有人都最合适。早期的卡片没有安排：已理解的按上次复习后第三档计，
//! 其余按加入时到期。

use crate::decision::DecisionVerdict;
use crate::decision::PracticeGoal;
use crate::review::ReviewAttempt;
use crate::review::ReviewCard;
use crate::review::ReviewStatus;

pub const DAY_MS: i64 = 86_400_000;

/// 一次复习的结果，用于安排间隔。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 没有走子，直接看了答案（或还没有复习过）。
    Revealed,
    /// 走了，但没有独立选对。
    Missed,
    /// 独立选对：复现棋谱与棋谱一致；局面决策与棋谱一致或与参考评分接近。
    Independent,
}

pub fn outcome_of(attempt: Option<&ReviewAttempt>) -> Outcome {
    let Some(a) = attempt.filter(|a| a.choice.is_some()) else { return Outcome::Revealed };
    let independent = match a.goal {
        PracticeGoal::Recall => a.matched,
        // 比较还没有结果或无法判断时，只有与棋谱一致才算独立选对
        PracticeGoal::Decision => match a.decision.as_ref().and_then(|d| d.verdict) {
            Some(DecisionVerdict::SameAsRecord | DecisionVerdict::Reasonable) => true,
            Some(DecisionVerdict::Noteworthy) => false,
            Some(DecisionVerdict::Unclear) | None => a.matched,
        },
    };
    if independent { Outcome::Independent } else { Outcome::Missed }
}

/// 标记状态之后的档位与间隔（天）。`intervals` 至少有一项（配置保证）。
pub fn next_review(stage: u32, status: ReviewStatus, outcome: Outcome, intervals: &[u32]) -> (u32, u32) {
    let days = |stage: u32| intervals[(stage as usize).min(intervals.len() - 1)];
    match (status, outcome) {
        (ReviewStatus::Understood, Outcome::Independent) => {
            let next = (stage + 1).min(intervals.len() as u32 - 1);
            (next, days(next))
        }
        (ReviewStatus::Understood, _) => (stage, days(0)),
        (ReviewStatus::Again, _) => (0, days(0)),
        // 待复习：立即到期
        (ReviewStatus::Due, _) => (0, 0),
    }
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
        // 无法判断或还没有结果：只有与棋谱一致才算独立选对
        assert_eq!(outcome_of(Some(&attempt(true, false, Decision, Some(Unclear)))), Outcome::Missed);
        assert_eq!(outcome_of(Some(&attempt(true, true, Decision, None))), Outcome::Independent);
    }
}
