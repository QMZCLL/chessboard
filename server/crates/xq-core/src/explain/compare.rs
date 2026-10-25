//! 同一局面下两种走法的对比：“我的选择”与“对照选择”（实际着法、引擎候选或棋谱中的其他分支）。
//!
//! 两步都从同一个比较起点出发。对比只回答四个问题：
//! 我的走法改变了什么、对照走法改变了什么（规则事实）；对手最值得关注的回应是什么（能合法回放的引擎变化）；
//! 两种选择有哪些已确认的差别（程序结论）。
//!
//! 评分来自对两步棋的同一次搜索（`searchmoves` 两步、两条候选），视角都是走棋方，深度与条件相同。
//! 搜索没有完成整轮、或评分只是搜索边界时，只给出区间能够证明的结论；证明不了时不比较优劣。
//! “两条变化都生成了”并不等于能够判断优劣。

use serde::Deserialize;
use serde::Serialize;

use super::compose::Claim;
use super::compose::EngineLine;
use super::compose::Evidence;
use super::compose::ExplainContext;
use super::compose::LineBase;
use super::compose::LineRef;
use super::compose::explain_move;
use super::compose::line_is_legal;
use super::compose::score_label;
use super::facts;
use crate::Move;
use crate::Position;
use crate::notation;
use crate::rules;
use crate::types::Color;
use crate::uci::Bound;
use crate::uci::Score;

/// 对比内容的生成版本：生成规则改变时递增。
pub const COMPARE_VERSION: u32 = 1;

/// 两种走法在同一次搜索中的结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareEvidence {
    /// 搜索给出的各条变化（评分为走棋方视角），按名次排列。
    pub lines: Vec<EngineLine>,
    /// 分析条件的文字说明。
    pub conditions: String,
    /// 两条变化来自同一深度的完整一轮。
    pub complete: bool,
    /// 搜索不完整的原因。
    pub notes: Vec<String>,
}

/// 对比的结论方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOutcome {
    /// 我的选择更好（区间能够证明）。
    MineBetter,
    /// 对照选择更好（区间能够证明）。
    ReferenceBetter,
    /// 两者接近。
    Close,
    /// 有分析结果，但不足以比较优劣。
    Unclear,
    /// 还没有两步棋的同一次分析。
    NotAnalyzed,
}

/// 对比中的一方。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareSide {
    pub mv: Move,
    pub notation: String,
    /// 这步改变了什么（规则事实）。
    pub changes: Vec<Claim>,
    /// 对手最值得关注的回应（来自能合法回放的引擎变化）。
    pub reply: Option<Claim>,
    /// 从比较起点开始的变化（第一着就是这一步），可以演示。
    pub line: Option<LineRef>,
    /// 程序给出的评分说明（带视角与边界）。
    pub score_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveComparison {
    pub generator_version: u32,
    /// 比较起点的行棋方（两步棋的走棋方）。
    pub mover: Color,
    pub mine: CompareSide,
    pub reference: CompareSide,
    pub outcome: CompareOutcome,
    /// 已确认的差别（程序结论）。
    pub differences: Vec<Claim>,
    /// 两边说明引用的证据（编号带 a-、b- 前缀区分两边）。
    pub evidence: Vec<Evidence>,
    pub warnings: Vec<String>,
    pub conditions: Option<String>,
}

/// 对比同一局面下的两种走法。走法不合法或两步相同时返回 `None`。
pub fn compare_moves(
    before: &Position, mine: Move, reference: Move, ctx: &ExplainContext, engine: Option<&CompareEvidence>,
) -> Option<MoveComparison> {
    if mine == reference || !rules::is_legal(before, mine) || !rules::is_legal(before, reference) {
        return None;
    }
    let mover = before.side;
    let mut evidence = Vec::new();
    let mut warnings = Vec::new();
    if !ctx.history_complete {
        warnings.push("此前的着法历史不完整，长将、长捉等依赖历史的判定可能不准确".to_string());
    }
    let mine_line = engine.and_then(|e| e.lines.iter().find(|l| l.mv == mine));
    let ref_line = engine.and_then(|e| e.lines.iter().find(|l| l.mv == reference));
    let a = side(before, mine, "a", ctx, mine_line, &mut evidence, &mut warnings);
    let b = side(before, reference, "b", ctx, ref_line, &mut evidence, &mut warnings);

    let mut differences = Vec::new();
    // 规则事实上的差别：是否吃子、是否将军
    let (fa, fb) = (facts::move_facts(before, mine)?, facts::move_facts(before, reference)?);
    if fa.captured.is_some() != fb.captured.is_some() || fa.captured != fb.captured {
        differences.push(fact_claim(capture_text(&a.notation, fa.captured.as_ref()), capture_text(&b.notation, fb.captured.as_ref())));
    }
    if fa.checkers.is_empty() != fb.checkers.is_empty() {
        let text = |name: &str, f: &facts::MoveFacts| {
            if f.checkers.is_empty() { format!("{name}不将军") } else { format!("{name}将军") }
        };
        differences.push(fact_claim(text(&a.notation, &fa), text(&b.notation, &fb)));
    }

    let outcome = match engine {
        None => {
            warnings.push("还没有两步棋的同一次分析，暂不比较优劣".to_string());
            CompareOutcome::NotAnalyzed
        }
        Some(e) => {
            warnings.extend(e.notes.iter().cloned());
            match (mine_line, ref_line) {
                _ if !e.complete => {
                    differences.push(program_claim("本次比较搜索没有完成整轮，暂不比较两步的优劣".to_string()));
                    CompareOutcome::Unclear
                }
                (Some(m), Some(r)) => {
                    let (outcome, text) = judge_pair(m, r, &a.notation, &b.notation);
                    differences.push(program_claim(format!("同一次搜索中，{text}")));
                    outcome
                }
                _ => {
                    differences.push(program_claim("搜索没有同时给出两步的结果，暂不比较优劣".to_string()));
                    CompareOutcome::Unclear
                }
            }
        }
    };
    // 对方主要应对的差别（引擎变化中的事实）
    if let (Some(ra), Some(rb)) = (reply_notation(before, mine_line), reply_notation(before, ref_line))
        && ra != rb
    {
        differences.push(Claim {
            text: format!("对方的主要应对不同：{}之后是{ra}，{}之后是{rb}", a.notation, b.notation),
            evidence: Vec::new(),
        });
    }

    Some(MoveComparison {
        generator_version: COMPARE_VERSION,
        mover,
        mine: a,
        reference: b,
        outcome,
        differences,
        evidence,
        warnings,
        conditions: engine.map(|e| e.conditions.clone()),
    })
}

/// 一方的说明：规则事实来自本着讲解的“为什么这样走”，证据编号加上前缀，避免与另一方冲突。
fn side(
    before: &Position, mv: Move, prefix: &str, ctx: &ExplainContext, line: Option<&EngineLine>, evidence: &mut Vec<Evidence>,
    warnings: &mut Vec<String>,
) -> CompareSide {
    let notation = notation::move_to_chinese(before, mv).unwrap_or_else(|_| mv.iccs());
    let mut changes = Vec::new();
    if let Some(expl) = explain_move(before, mv, ctx, None) {
        let used: std::collections::HashSet<&String> = expl.basis.iter().flat_map(|c| c.evidence.iter()).collect();
        for ev in expl.evidence.iter().filter(|e| used.contains(&e.id)) {
            evidence.push(Evidence { id: format!("{prefix}-{}", ev.id), ..ev.clone() });
        }
        changes = expl
            .basis
            .iter()
            .map(|c| Claim { text: c.text.clone(), evidence: c.evidence.iter().map(|id| format!("{prefix}-{id}")).collect() })
            .collect();
    }
    let legal = line.filter(|l| line_is_legal(before, &l.pv) && l.pv.first() == Some(&mv));
    if line.is_some() && legal.is_none() {
        warnings.push(format!("{notation}的引擎变化无法合法回放，已移除"));
    }
    let reply = legal.and_then(|l| {
        let after = before.after(mv);
        let r = *l.pv.get(1)?;
        let name = notation::move_to_chinese(&after, r).ok()?;
        Some(Claim { text: format!("对方最值得关注的回应是{name}"), evidence: Vec::new() })
    });
    CompareSide {
        mv,
        notation,
        changes,
        reply,
        line: legal.map(|l| LineRef { base: LineBase::Parent, moves: l.pv.clone(), notation: notation::line_to_chinese(before, &l.pv) }),
        score_text: line.map(|l| score_label(l.score, l.bound, before.side)),
    }
}

fn reply_notation(before: &Position, line: Option<&EngineLine>) -> Option<String> {
    let l = line?;
    if !line_is_legal(before, &l.pv) || l.pv.len() < 2 {
        return None;
    }
    notation::move_to_chinese(&before.after(l.pv[0]), l.pv[1]).ok()
}

fn capture_text(name: &str, captured: Option<&crate::types::Piece>) -> String {
    match captured {
        Some(p) => format!("{name}吃掉对方的{}", p.name()),
        None => format!("{name}不吃子"),
    }
}

fn fact_claim(a: String, b: String) -> Claim { Claim { text: format!("{a}；{b}"), evidence: Vec::new() } }

fn program_claim(text: String) -> Claim { Claim { text, evidence: Vec::new() } }

/// 普通分的取值区间：精确分为单点，下界只知道不低于该值，上界只知道不高于该值。
fn interval(line: &EngineLine) -> Option<(Option<i32>, Option<i32>)> {
    let cp = line.score.cp()?;
    Some(match line.bound {
        Bound::Exact => (Some(cp), Some(cp)),
        Bound::Lower => (Some(cp), None),
        Bound::Upper => (None, Some(cp)),
    })
}

fn gap_word(d: i32) -> &'static str {
    match d {
        i32::MIN..=80 => "略逊",
        81..=200 => "明显不如",
        _ => "差距很大",
    }
}

/// 比较两条变化（评分都是走棋方视角）。
///
/// 将杀分与普通分不相减；涉及搜索边界时把评分当作区间，差值区间为
/// [我的下界 - 对照上界, 我的上界 - 对照下界]，只给出区间能证明的结论。
pub fn judge_pair(mine: &EngineLine, reference: &EngineLine, a: &str, b: &str) -> (CompareOutcome, String) {
    use CompareOutcome::*;
    let exact = mine.bound == Bound::Exact && reference.bound == Bound::Exact;
    if mine.score.is_mate() || reference.score.is_mate() {
        if !exact {
            return (Unclear, "涉及将杀分的搜索边界值，需要继续搜索才能比较".to_string());
        }
        return match (mine.score, reference.score) {
            // 普通评分只说明这次搜索没有发现强制杀棋，不能证明以后不会被杀：结论都限定为本次搜索的结果
            (Score::Mate(x), Score::Mate(y)) if x > 0 && y > 0 => match x.cmp(&y) {
                std::cmp::Ordering::Equal => (Close, format!("本次搜索中两步都能在 {x} 步内杀棋")),
                std::cmp::Ordering::Less => (MineBetter, format!("本次搜索中两步都能杀棋，{a}更快（{x} 步对 {y} 步）")),
                std::cmp::Ordering::Greater => (ReferenceBetter, format!("本次搜索中两步都能杀棋，{b}更快（{y} 步对 {x} 步）")),
            },
            (Score::Mate(x), _) if x > 0 => {
                (MineBetter, format!("本次搜索发现{a}可以在 {x} 步内杀棋；{b}的搜索结果未发现杀棋"))
            }
            (_, Score::Mate(y)) if y > 0 => {
                (ReferenceBetter, format!("本次搜索发现{b}可以在 {y} 步内杀棋；{a}的搜索结果未发现杀棋"))
            }
            (Score::Mate(x), Score::Mate(y)) => {
                (Close, format!("本次搜索发现两步之后都会被对方杀棋（{} 步与 {} 步）", -x, -y))
            }
            (Score::Mate(x), _) => (
                ReferenceBetter,
                format!("本次搜索发现{a}之后会被对方在 {} 步内杀棋；{b}的搜索结果未发现同类强制杀棋", -x),
            ),
            (_, Score::Mate(y)) => (
                MineBetter,
                format!("本次搜索发现{b}之后会被对方在 {} 步内杀棋；{a}的搜索结果未发现同类强制杀棋", -y),
            ),
            _ => (Unclear, "将杀判断不一致".to_string()),
        };
    }
    let (Some((m_lo, m_hi)), Some((r_lo, r_hi))) = (interval(mine), interval(reference)) else {
        return (Unclear, "评分无法比较".to_string());
    };
    if exact {
        let d = m_lo.unwrap_or(0) - r_lo.unwrap_or(0);
        return if d.abs() <= 30 {
            (Close, format!("{a}与{b}评分接近（相差约 {} 分）", d.abs()))
        } else if d > 0 {
            (MineBetter, format!("{b}比{a}低约 {d} 分（{}）", gap_word(d)))
        } else {
            (ReferenceBetter, format!("{a}比{b}低约 {} 分（{}）", -d, gap_word(-d)))
        };
    }
    let lo = m_lo.zip(r_hi).map(|(m, r)| m - r);
    let hi = m_hi.zip(r_lo).map(|(m, r)| m - r);
    match (lo, hi) {
        (Some(l), _) if l > 30 => (MineBetter, format!("按搜索边界值，{b}至少比{a}低约 {l} 分")),
        (_, Some(h)) if h < -30 => (ReferenceBetter, format!("按搜索边界值，{a}至少比{b}低约 {} 分", -h)),
        (Some(l), Some(h)) if l >= -30 && h <= 30 => (Close, format!("按搜索边界值，{a}与{b}相差不超过 30 分")),
        _ => (Unclear, "评分是搜索边界值，区间相互重叠，无法确定差距".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_position;

    const START: &str = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w";
    const CTX: ExplainContext = ExplainContext { history_complete: true, repetition_count: 1 };

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    fn line(m: &str, score: Score, bound: Bound, pv: &[&str]) -> EngineLine {
        EngineLine { mv: mv(m), score, bound, depth: 12, pv: pv.iter().map(|s| mv(s)).collect() }
    }

    fn engine(lines: Vec<EngineLine>, complete: bool) -> CompareEvidence {
        CompareEvidence { lines, conditions: "测试 · 深度 12".into(), complete, notes: vec![] }
    }

    #[test]
    fn mate_against_one_side_is_limited_to_what_this_search_found() {
        let mated = line("h2e2", Score::Mate(-4), Bound::Exact, &["h2e2"]);
        let normal = line("c3c4", Score::Cp(0), Bound::Exact, &["c3c4"]);
        for (outcome, text) in [judge_pair(&mated, &normal, "甲", "乙"), judge_pair(&normal, &mated, "乙", "甲")] {
            assert!(matches!(outcome, CompareOutcome::ReferenceBetter | CompareOutcome::MineBetter));
            assert!(text.contains("本次搜索发现甲之后会被对方在 4 步内杀棋"), "{text}");
            assert!(text.contains("乙的搜索结果未发现同类强制杀棋"), "{text}");
            assert!(!text.contains("不会") && !text.contains("一定") && !text.contains("避免"), "普通评分不能证明不会被杀：{text}");
        }
        let winning = line("h2e2", Score::Mate(3), Bound::Exact, &["h2e2"]);
        let (_, text) = judge_pair(&winning, &normal, "甲", "乙");
        assert!(text.contains("本次搜索发现甲可以在 3 步内杀棋") && text.contains("乙的搜索结果未发现杀棋"), "{text}");
    }

    #[test]
    fn both_sides_from_the_same_start_with_rule_facts_replies_and_lines() {
        let pos = parse_position(START).unwrap();
        let e = engine(
            vec![line("h2e2", Score::Cp(40), Bound::Exact, &["h2e2", "h9g7"]), line("c3c4", Score::Cp(20), Bound::Exact, &["c3c4", "h7e7"])],
            true,
        );
        let c = compare_moves(&pos, mv("c3c4"), mv("h2e2"), &CTX, Some(&e)).unwrap();
        assert_eq!(c.mine.notation, "兵七进一");
        assert_eq!(c.reference.notation, "炮二平五");
        assert!(!c.mine.changes.is_empty() && !c.reference.changes.is_empty());
        assert_eq!(c.mine.reply.as_ref().unwrap().text, "对方最值得关注的回应是炮8平5");
        // 两条变化都从比较起点开始，第一着就是各自的走法
        let l = c.reference.line.as_ref().unwrap();
        assert_eq!((l.base, l.moves[0]), (LineBase::Parent, mv("h2e2")));
        assert_eq!(c.outcome, CompareOutcome::Close);
        assert!(c.differences.iter().any(|d| d.text.contains("评分接近")), "{:?}", c.differences);
        assert!(c.differences.iter().any(|d| d.text.contains("对方的主要应对不同")));
        // 两边的证据编号互不冲突，引用都能找到
        for claim in c.mine.changes.iter().chain(&c.reference.changes) {
            for id in &claim.evidence {
                assert!(c.evidence.iter().any(|e| &e.id == id), "缺少证据 {id}");
            }
        }
        assert!(c.mine.score_text.as_ref().unwrap().contains("红方视角评分 +20"));
    }

    #[test]
    fn exact_scores_give_a_direction_only_beyond_the_threshold() {
        let a = line("c3c4", Score::Cp(-120), Bound::Exact, &["c3c4"]);
        let b = line("h2e2", Score::Cp(40), Bound::Exact, &["h2e2"]);
        let (o, t) = judge_pair(&a, &b, "我的", "对照");
        assert_eq!(o, CompareOutcome::ReferenceBetter);
        assert!(t.contains("低约 160 分（明显不如）"), "{t}");
        let (o, _) = judge_pair(&b, &a, "我的", "对照");
        assert_eq!(o, CompareOutcome::MineBetter);
    }

    #[test]
    fn bounds_only_give_conclusions_the_interval_proves() {
        // 我的不高于 +10，对照不低于 +80：至少低 70 分
        let a = line("c3c4", Score::Cp(10), Bound::Upper, &["c3c4"]);
        let b = line("h2e2", Score::Cp(80), Bound::Lower, &["h2e2"]);
        assert_eq!(judge_pair(&a, &b, "我的", "对照").0, CompareOutcome::ReferenceBetter);
        // 区间重叠：无法确定
        let a = line("c3c4", Score::Cp(10), Bound::Lower, &["c3c4"]);
        let b = line("h2e2", Score::Cp(80), Bound::Exact, &["h2e2"]);
        assert_eq!(judge_pair(&a, &b, "我的", "对照").0, CompareOutcome::Unclear);
        // 将杀分的边界值不比较
        let a = line("c3c4", Score::Mate(3), Bound::Upper, &["c3c4"]);
        assert_eq!(judge_pair(&a, &b, "我的", "对照").0, CompareOutcome::Unclear);
    }

    #[test]
    fn mate_scores_are_compared_by_meaning_not_subtracted() {
        let a = line("c3c4", Score::Mate(2), Bound::Exact, &["c3c4"]);
        let b = line("h2e2", Score::Cp(900), Bound::Exact, &["h2e2"]);
        assert_eq!(judge_pair(&a, &b, "我的", "对照").0, CompareOutcome::MineBetter);
        let a = line("c3c4", Score::Mate(-4), Bound::Exact, &["c3c4"]);
        let (o, t) = judge_pair(&a, &b, "我的", "对照");
        assert_eq!(o, CompareOutcome::ReferenceBetter);
        assert!(t.contains("4 步内杀棋"), "{t}");
    }

    #[test]
    fn incomplete_or_missing_analysis_does_not_judge() {
        let pos = parse_position(START).unwrap();
        let lines = vec![line("h2e2", Score::Cp(40), Bound::Exact, &["h2e2"]), line("c3c4", Score::Cp(-300), Bound::Exact, &["c3c4"])];
        let c = compare_moves(&pos, mv("c3c4"), mv("h2e2"), &CTX, Some(&engine(lines, false))).unwrap();
        assert_eq!(c.outcome, CompareOutcome::Unclear);
        assert!(c.differences.iter().all(|d| !d.text.contains("低约")), "{:?}", c.differences);
        let c = compare_moves(&pos, mv("c3c4"), mv("h2e2"), &CTX, None).unwrap();
        assert_eq!(c.outcome, CompareOutcome::NotAnalyzed);
        assert!(c.mine.score_text.is_none() && c.mine.line.is_none());
        assert!(!c.mine.changes.is_empty(), "没有引擎时仍给出规则事实");
    }

    #[test]
    fn illegal_lines_are_removed_and_same_or_illegal_moves_are_rejected() {
        let pos = parse_position(START).unwrap();
        // 对照的变化第二着不合法（红方连走两步）
        let lines = vec![line("h2e2", Score::Cp(40), Bound::Exact, &["h2e2", "b2e2"]), line("c3c4", Score::Cp(20), Bound::Exact, &["c3c4"])];
        let c = compare_moves(&pos, mv("c3c4"), mv("h2e2"), &CTX, Some(&engine(lines, true))).unwrap();
        assert!(c.reference.line.is_none() && c.reference.reply.is_none());
        assert!(c.warnings.iter().any(|w| w.contains("无法合法回放")));
        assert!(compare_moves(&pos, mv("h2e2"), mv("h2e2"), &CTX, None).is_none());
        assert!(compare_moves(&pos, mv("h2e2"), mv("a0a5"), &CTX, None).is_none());
    }

    #[test]
    fn rule_differences_name_captures() {
        // 红炮可以打中卒，也可以平炮
        let pos = parse_position("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKAB1R w").unwrap();
        let c = compare_moves(&pos, mv("e2e6"), mv("b2c2"), &CTX, None).unwrap();
        assert!(c.differences.iter().any(|d| d.text.contains("吃掉对方的卒") && d.text.contains("不吃子")), "{:?}", c.differences);
    }
}
