//! 搜索结果的稳定性。
//!
//! 只看一次搜索最后一轮的排名，看不出结论是否还在变化：最近几个深度中首选可能刚刚换过，
//! 前几名的次序可能来回交换，评分可能仍在单向变化。这里按最近几个深度的完整一轮判断，分为
//! 高、中、仍在变化、不完整四级，并写明依据。它衡量的是这次搜索的结论稳不稳定，
//! 不是走法正确的概率，也没有经过校准，不换算成百分比。
//!
//! 用途：讲解、两步对比与局面决策在“仍在变化”时不评价排名与优劣（见 `restrains`）；
//! 讲解在快速分析仍在变化时补充一次深入分析。

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use crate::board::Position;
use crate::notation;
use crate::types::Move;
use crate::uci::Bound;
use crate::uci::Score;

/// 一个深度上完整的一轮候选：首着按名次排列，以及首选的评分。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundSummary {
    pub depth: u32,
    pub moves: Vec<Move>,
    pub top: Score,
    pub top_bound: Bound,
}

/// 稳定性等级，按从弱到强排列。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum StabilityLevel {
    /// 候选比较没有完成一轮完整的搜索。
    Incomplete,
    /// 最近的深度中首选还在变化：不评价排名与优劣。
    Shifting,
    /// 结论大体稳定，但有需要注意的地方（见依据）。
    Medium,
    High,
}

impl StabilityLevel {
    pub fn label(self) -> &'static str {
        match self {
            StabilityLevel::Incomplete => "分析不完整",
            StabilityLevel::Shifting => "仍在变化",
            StabilityLevel::Medium => "中等稳定",
            StabilityLevel::High => "高稳定",
        }
    }
}

/// 搜索结论的稳定性：衡量这次搜索的结论还在不在变化，不是正确的概率。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Stability {
    pub level: StabilityLevel,
    /// 判断的依据，按重要程度排列。
    pub reasons: Vec<String>,
    /// 参与判断的深度范围（最浅、最深）。
    pub depths: Option<(u32, u32)>,
}

impl Stability {
    /// 排名与优劣的结论是否需要收住：仍在变化或不完整时不说“首选”“更好”“明显不如”等。
    pub fn restrains(&self) -> bool { self.level <= StabilityLevel::Shifting }
}

/// 参与判断的最近几个深度。
pub const WINDOW: usize = 4;
/// 首选评分在窗口内单向变化超过这个分数时，认为评分仍在漂移。
pub const DRIFT_CP: i32 = 60;
/// 少于这个数目的完整深度时，看不出排名是否稳定。
const MIN_ROUNDS: usize = 3;

fn name(pos: &Position, mv: Move) -> String { notation::move_to_chinese(pos, mv).unwrap_or_else(|_| mv.iccs()) }

/// 按最近几个深度的完整一轮判断稳定性。`rounds` 按深度升序；`complete` 为这次搜索是否给出了完整一致的候选。
pub fn assess(pos: &Position, rounds: &[RoundSummary], complete: bool) -> Stability {
    let rounds: Vec<&RoundSummary> = rounds.iter().filter(|r| !r.moves.is_empty()).collect();
    if !complete {
        return Stability {
            level: StabilityLevel::Incomplete,
            reasons: vec!["候选比较没有完成一轮完整的搜索".to_string()],
            depths: rounds.last().map(|r| (r.depth, r.depth)),
        };
    }
    let window = &rounds[rounds.len().saturating_sub(WINDOW)..];
    let Some(last) = window.last() else {
        return Stability {
            level: StabilityLevel::Medium,
            reasons: vec!["没有记录各个深度的结果，看不出排名是否稳定".to_string()],
            depths: None,
        };
    };
    let depths = Some((window[0].depth, last.depth));
    let range = format!("深度 {}–{}", window[0].depth, last.depth);
    let best = name(pos, last.moves[0]);

    // 首选的变化
    let changes: Vec<(u32, Move, Move)> =
        window.windows(2).filter(|w| w[0].moves[0] != w[1].moves[0]).map(|w| (w[1].depth, w[0].moves[0], w[1].moves[0])).collect();
    let just_changed = window.len() >= 2 && window[window.len() - 2].moves[0] != last.moves[0];
    if just_changed || changes.len() >= 2 {
        let mut seen: Vec<Move> = Vec::new();
        for r in window {
            if !seen.contains(&r.moves[0]) {
                seen.push(r.moves[0]);
            }
        }
        let names: Vec<String> = seen.iter().map(|m| name(pos, *m)).collect();
        let reason = if just_changed {
            format!("最后一个深度（{}）首选才由{}换成{}；{range} 中首选先后是{}", last.depth, name(pos, window[window.len() - 2].moves[0]), best, names.join("、"))
        } else {
            format!("{range} 中首选在{}之间变化了 {} 次", names.join("、"), changes.len())
        };
        return Stability { level: StabilityLevel::Shifting, reasons: vec![reason], depths };
    }

    let mut reasons = Vec::new();
    if let Some((depth, from, to)) = changes.first() {
        reasons.push(format!("首选在深度 {depth} 由{}换成{}，之后没有再变", name(pos, *from), name(pos, *to)));
    }
    // 前几名（最多三名）的次序变化
    let swaps = window
        .windows(2)
        .filter(|w| {
            let n = w[0].moves.len().min(w[1].moves.len()).min(3);
            n >= 2 && w[0].moves[..n] != w[1].moves[..n]
        })
        .count();
    if swaps >= 2 {
        reasons.push(format!("{range} 中前几名的次序变化了 {swaps} 次"));
    }
    // 首选评分的单向漂移（只看普通分的精确值）
    let cps: Vec<i32> = window.iter().filter(|r| r.top_bound == Bound::Exact).filter_map(|r| r.top.cp()).collect();
    if cps.len() == window.len() && cps.len() >= 2 {
        let rising = cps.windows(2).all(|w| w[1] >= w[0]);
        let falling = cps.windows(2).all(|w| w[1] <= w[0]);
        let moved = cps[cps.len() - 1] - cps[0];
        if (rising || falling) && moved.abs() > DRIFT_CP {
            let way = if moved > 0 { "上升" } else { "下降" };
            reasons.push(format!("首选的评分在{range} 中持续{way}约 {} 分", moved.abs()));
        }
    }
    if last.top_bound != Bound::Exact {
        reasons.push("最后一个深度首选的评分只是搜索边界值".to_string());
    }
    if window.len() < MIN_ROUNDS {
        reasons.push(format!("只完成了 {} 个深度的完整一轮，看不出排名是否稳定", window.len()));
    }
    if reasons.is_empty() {
        let n = window.len();
        return Stability {
            level: StabilityLevel::High,
            reasons: vec![format!("{range}（最近 {n} 个深度）首选都是{best}，前几名的次序没有变化")],
            depths,
        };
    }
    Stability { level: StabilityLevel::Medium, reasons, depths }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Position;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    fn round(depth: u32, moves: &[&str], cp: i32) -> RoundSummary {
        RoundSummary { depth, moves: moves.iter().map(|m| mv(m)).collect(), top: Score::Cp(cp), top_bound: Bound::Exact }
    }

    const A: &str = "h2e2";
    const B: &str = "h0g2";
    const C: &str = "b2e2";

    #[test]
    fn a_settled_search_is_high_and_says_why() {
        let pos = Position::startpos();
        let rounds: Vec<_> = (7..=12).map(|d| round(d, &[A, B, C], 30)).collect();
        let s = assess(&pos, &rounds, true);
        assert_eq!((s.level, s.depths), (StabilityLevel::High, Some((9, 12))));
        assert!(s.reasons[0].contains("首选都是炮二平五"), "{:?}", s.reasons);
        assert!(!s.restrains());
    }

    #[test]
    fn a_best_move_that_just_changed_or_keeps_changing_is_shifting() {
        let pos = Position::startpos();
        let just = [round(9, &[A, B], 30), round(10, &[A, B], 31), round(11, &[A, B], 30), round(12, &[B, A], 33)];
        let s = assess(&pos, &just, true);
        assert_eq!(s.level, StabilityLevel::Shifting);
        assert!(s.reasons[0].contains("最后一个深度（12）首选才由炮二平五换成马二进三"), "{:?}", s.reasons);
        assert!(s.restrains());
        let back_and_forth = [round(9, &[A, B], 30), round(10, &[B, A], 31), round(11, &[A, B], 30), round(12, &[A, B], 30)];
        let s = assess(&pos, &back_and_forth, true);
        assert_eq!(s.level, StabilityLevel::Shifting);
        assert!(s.reasons[0].contains("变化了 2 次"), "{:?}", s.reasons);
    }

    #[test]
    fn one_early_change_swaps_drift_bounds_and_few_rounds_are_medium() {
        let pos = Position::startpos();
        let once = [round(9, &[B, A], 30), round(10, &[A, B], 31), round(11, &[A, B], 30), round(12, &[A, B], 30)];
        let s = assess(&pos, &once, true);
        assert_eq!(s.level, StabilityLevel::Medium);
        assert!(s.reasons[0].contains("首选在深度 10 由马二进三换成炮二平五"), "{:?}", s.reasons);
        let swaps = [round(9, &[A, B, C], 30), round(10, &[A, C, B], 30), round(11, &[A, B, C], 30), round(12, &[A, B, C], 30)];
        assert!(assess(&pos, &swaps, true).reasons[0].contains("前几名的次序变化了 2 次"));
        let drift = [round(9, &[A, B], 10), round(10, &[A, B], 40), round(11, &[A, B], 70), round(12, &[A, B], 95)];
        let s = assess(&pos, &drift, true);
        assert_eq!(s.level, StabilityLevel::Medium);
        assert!(s.reasons[0].contains("持续上升约 85 分"), "{:?}", s.reasons);
        let mut bounded = round(12, &[A, B], 30);
        bounded.top_bound = Bound::Lower;
        let s = assess(&pos, &[round(10, &[A, B], 30), round(11, &[A, B], 30), bounded], true);
        assert!(s.reasons.iter().any(|r| r.contains("边界值")), "{:?}", s.reasons);
        let s = assess(&pos, &[round(11, &[A, B], 30), round(12, &[A, B], 30)], true);
        assert_eq!(s.level, StabilityLevel::Medium);
        assert!(s.reasons[0].contains("只完成了 2 个深度"));
    }

    #[test]
    fn an_incomplete_search_or_missing_rounds_are_said_plainly() {
        let pos = Position::startpos();
        let s = assess(&pos, &[round(12, &[A], 30)], false);
        assert_eq!(s.level, StabilityLevel::Incomplete);
        assert!(s.restrains());
        let s = assess(&pos, &[], true);
        assert_eq!((s.level, s.depths), (StabilityLevel::Medium, None));
        assert!(s.reasons[0].contains("没有记录"));
        // 将杀分不参与评分漂移的判断
        let mate: Vec<_> = (9..=12).map(|d| RoundSummary { top: Score::Mate(3), ..round(d, &[A, B], 0) }).collect();
        assert_eq!(assess(&pos, &mate, true).level, StabilityLevel::High);
    }
}
