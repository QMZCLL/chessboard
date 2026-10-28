//! 复习时的“局面决策”：把这次的选择、棋谱中的着法与引擎参考放在同一次搜索中比较，
//! 并把落子前选择的意图与这一步实际的规则事实放在一起。
//!
//! - 复现棋谱只看是否与棋谱相同；局面决策承认与棋谱不同的合理选择：棋谱中的着法也可能不是好的参考，
//!   先用多候选搜索找出引擎参考，再对三步做同一次搜索（`searchmoves`），按同一深度的评分比较。
//! - 反馈只有四种：与棋谱一致；与棋谱不同，但与参考评分接近；留下了值得注意的问题；分析尚不充分。
//!   “与棋谱一致”只陈述匹配的事实；这次比较是否支持这个选择另外记录（`DecisionSupport`）：
//!   棋谱着法在同一次搜索中明显不如引擎参考时，走了棋谱着法也不算决策正确。
//!   评分、边界与将杀分沿用试走对比的判断（`judge_pair`），不完整的搜索不评价优劣。
//!   对比中“略逊”的分差（见 `NOTEWORTHY_GAP`）在练习中仍算与参考接近：快速搜索里这样的差距不足以说明选择有问题。
//! - 意图对照只用规则事实（吃子、将军、出子、受攻击的棋子、保护关系），不推测用户的想法。
//! - 搜索在后台进行，完成后写回这次复习并推送 `review_updated`；卡片的局面与着法建立后不再改变。

use serde::Deserialize;
use serde::Serialize;
use tracing::warn;
use xq_core::Move;
use xq_core::Position;
use xq_core::explain::EngineLine;
use xq_core::explain::ExplainContext;
use xq_core::explain::compare::CompareOutcome;
use xq_core::explain::compare::judge_pair;
use xq_core::explain::describe::join_labels;
use xq_core::explain::describe::label_at;
use xq_core::explain::describe::piece_label;
use xq_core::explain::explain_move;
use xq_core::explain::facts;
use xq_core::explain::facts::Development;
use xq_core::explain::line_is_legal;
use xq_core::fen;
use xq_core::notation;
use xq_core::rules;

use crate::engine::Priority;
use crate::engine::SearchOutcome;
use crate::events;
use crate::records::AnalysisConditions;
use crate::records::Budget;
use crate::review::ReviewCard;
use crate::review::ReviewMove;
use crate::review::ReviewStatus;
use crate::schedule;
use crate::service::now_ms;
use crate::service::ApiResult;
use crate::service::Inner;

/// 普通分的差距超过它才算“留下了值得注意的问题”；与试走对比中“略逊”的上限相同。
const NOTEWORTHY_GAP: i32 = 80;

/// 两条变化都是精确的普通分时，参考比这次的选择高出多少分。
fn exact_gap(choice: &EngineLine, reference: &EngineLine) -> Option<i32> {
    let exact = choice.bound == xq_core::uci::Bound::Exact && reference.bound == xq_core::uci::Bound::Exact;
    exact.then(|| Some(reference.score.cp()? - choice.score.cp()?)).flatten()
}

/// 这次复习的练习目标。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PracticeGoal {
    /// 复现棋谱：回忆这盘棋当时怎样走，看是否与棋谱一致。
    #[default]
    Recall,
    /// 局面决策：在这个局面中作出自己的选择，与参考在同一次搜索中比较。
    Decision,
}

/// 落子前选择的意图：这一步主要想做什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Develop,
    Defend,
    Capture,
    Check,
    Improve,
    Other,
}

impl Intent {
    pub fn label(self) -> &'static str {
        match self {
            Intent::Develop => "出子",
            Intent::Defend => "防守",
            Intent::Capture => "吃子",
            Intent::Check => "将军",
            Intent::Improve => "改善位置",
            Intent::Other => "其他",
        }
    }
}

/// 这一步实际改变了什么（规则事实），以及与落子前意图的对照。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reflection {
    pub intent: Option<Intent>,
    /// 与意图对照的规则事实；没有选择意图、或意图为“其他”时为空。
    pub checks: Vec<String>,
    /// 这一步改变了什么（与讲解中“为什么这样走”相同来源的规则事实）。
    pub changes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    /// 搜索还在进行。
    Pending,
    Ready,
    /// 没有得到搜索结果（引擎不可用、搜索失败等）。
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionVerdict {
    /// 与棋谱一致。
    SameAsRecord,
    /// 与棋谱不同，但本次搜索中与参考评分接近（或不低于参考）。
    Reasonable,
    /// 本次搜索中明显低于参考：这个选择留下了一个值得注意的问题。
    Noteworthy,
    /// 分析尚不充分，暂不评价走法优劣。
    Unclear,
}

/// 这次比较是否支持这个选择：局面决策的评价依据，与“是否与棋谱一致”分开。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSupport {
    /// 与参考评分接近，或就是参考。
    Supported,
    /// 明显不如参考（包括走了在同一次搜索中明显不如引擎参考的棋谱着法）。
    NotSupported,
    /// 比较还没有结果、分析不完整或没有得到结果。
    Unknown,
    /// 早期的记录没有这一项，读取时按结论推算（见 `DecisionFeedback::support_for`）。
    #[default]
    Legacy,
}

/// 局面决策的反馈。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionFeedback {
    pub state: DecisionState,
    pub verdict: Option<DecisionVerdict>,
    #[serde(default)]
    pub support: DecisionSupport,
    /// 用来对照的着法：棋谱中的着法，或本次搜索中明显好于它的引擎参考。
    pub reference: Option<ReviewMove>,
    /// 程序给出的说明：同一次搜索的评分比较、对方的回应、分析是否完整。
    pub points: Vec<String>,
    pub conditions: Option<String>,
    pub error: Option<String>,
}

impl DecisionFeedback {
    fn pending() -> Self {
        Self {
            state: DecisionState::Pending,
            verdict: None,
            support: DecisionSupport::Unknown,
            reference: None,
            points: Vec::new(),
            conditions: None,
            error: None,
        }
    }

    /// 这次比较是否支持 `choice`；早期的记录按结论与对照着法推算。
    pub fn support_for(&self, choice: Option<Move>) -> DecisionSupport {
        if self.support != DecisionSupport::Legacy {
            return self.support;
        }
        if self.state != DecisionState::Ready {
            return DecisionSupport::Unknown;
        }
        match self.verdict {
            Some(DecisionVerdict::Reasonable) => DecisionSupport::Supported,
            Some(DecisionVerdict::Noteworthy) => DecisionSupport::NotSupported,
            // 对照着法换成了明显更好的引擎参考，说明棋谱着法不是好的选择
            Some(DecisionVerdict::SameAsRecord) => match (&self.reference, choice) {
                (Some(r), Some(c)) if r.mv == c => DecisionSupport::Supported,
                (Some(_), Some(_)) => DecisionSupport::NotSupported,
                _ => DecisionSupport::Unknown,
            },
            Some(DecisionVerdict::Unclear) | None => DecisionSupport::Unknown,
        }
    }

    fn failed(error: String) -> Self { Self { state: DecisionState::Failed, error: Some(error), ..Self::pending() } }
}

/// 一组受攻击棋子的称呼。
fn labels(board: &xq_core::Board, squares: impl Iterator<Item = xq_core::Square>) -> String {
    join_labels(&squares.map(|sq| label_at(board, sq)).collect::<Vec<_>>())
}

/// 一种威胁的说明。
pub(crate) fn threat_kind(t: &facts::Threat) -> &'static str {
    if t.defended { "受到价值更低的棋子攻击（能吃回，但交换吃亏）" } else { "受到攻击、缺少保护" }
}

/// 与意图对照的规则事实。
fn intent_checks(intent: Intent, f: &facts::MoveFacts) -> Vec<String> {
    let piece = piece_label(f.piece, f.mv.from);
    let mut out = Vec::new();
    match intent {
        Intent::Capture => out.push(match f.captured {
            Some(p) => format!("这一步吃掉了对方的{}", p.name()),
            None => "这一步没有吃子".to_string(),
        }),
        Intent::Check => out.push(if f.checkers.is_empty() { "这一步没有将军".to_string() } else { "这一步将军".to_string() }),
        Intent::Develop => out.push(match f.development {
            Some(Development::LeavesHome) => format!("这一步把{piece}从底线走出"),
            Some(Development::CrossesRiver) => "这一步让兵（卒）过河".to_string(),
            Some(Development::CenterCannon) => "这一步把炮架到中路".to_string(),
            Some(Development::Defensive) => format!("这一步走的是{piece}，属于防守的调整，不是出动车马炮"),
            Some(Development::KingMove) => "这一步走的是将帅，不是出动子力".to_string(),
            None => "这一步走的不是还在底线的车、马，也不是过河兵（卒）或中炮".to_string(),
        }),
        Intent::Defend => {
            if f.was_in_check {
                out.push("走之前正被将军，这一步应将".to_string());
            }
            let (before, after) = (&f.before.board, &f.after.board);
            if f.threats_before.is_empty() && !f.was_in_check {
                out.push("走之前没有己方棋子直接受到威胁".to_string());
            }
            // 威胁有两种（见 `facts::en_prise`）：被吃后不能吃回；能吃回，但攻击者价值更低、交换吃亏
            for t in &f.threats_before {
                // 走动的棋子换了位置：按它的新位置判断是否仍受威胁
                let at = if t.square == f.mv.from { f.mv.to } else { t.square };
                match f.threats_after.iter().find(|n| n.square == at) {
                    None => out.push(format!("{}原本{}，现在不再受到这个威胁", label_at(before, t.square), threat_kind(t))),
                    Some(now) => out.push(format!("{}仍然{}", label_at(after, at), threat_kind(now))),
                }
            }
            if !f.new_protects.is_empty() {
                out.push(format!("这一步保护了{}", labels(after, f.new_protects.iter().map(|r| r.target))));
            }
        }
        Intent::Improve => {
            out.push(format!("{piece}能走到的位置从 {} 处变为 {} 处", f.mobility_before, f.mobility_after));
            if !f.new_attacks.is_empty() {
                out.push(format!("这一步新攻击了对方的{}", labels(&f.after.board, f.new_attacks.iter().map(|r| r.target))));
            }
        }
        Intent::Other => {}
    }
    out
}

/// 这一步实际改变了什么，以及与意图的对照（只用规则事实）。
pub fn reflect(before: &Position, choice: Move, intent: Option<Intent>, ctx: &ExplainContext) -> Reflection {
    let changes = explain_move(before, choice, ctx, None).map(|e| e.basis.iter().map(|c| c.text.clone()).collect()).unwrap_or_default();
    let checks = match (intent, facts::move_facts(before, choice)) {
        (Some(i), Some(f)) => intent_checks(i, &f),
        _ => Vec::new(),
    };
    Reflection { intent, checks, changes }
}

fn name(pos: &Position, mv: Move) -> String { notation::move_to_chinese(pos, mv).unwrap_or_else(|_| mv.iccs()) }

fn review_move(pos: &Position, mv: Move) -> ReviewMove { ReviewMove { mv, notation: name(pos, mv) } }

/// 按同一次搜索的结果给出反馈。`best` 是多候选搜索的首位（与棋谱着法相同或没有结果时为空），
/// `lines` 来自对这次的选择、棋谱着法与引擎参考的同一次搜索。
pub fn judge_decision(
    before: &Position, choice: Move, answer: Move, best: Option<Move>, lines: &[EngineLine], complete: bool,
) -> DecisionFeedback {
    let line = |mv: Move| lines.iter().find(|l| l.mv == mv && l.pv.first() == Some(&mv) && line_is_legal(before, &l.pv));
    let ready = |verdict: DecisionVerdict, support: DecisionSupport, reference: Move, points: Vec<String>| DecisionFeedback {
        state: DecisionState::Ready,
        verdict: Some(verdict),
        support,
        reference: Some(review_move(before, reference)),
        points,
        conditions: None,
        error: None,
    };
    if !complete {
        // 与棋谱一致是事实，但棋谱着法的好坏也没有得到比较
        let verdict = if choice == answer { DecisionVerdict::SameAsRecord } else { DecisionVerdict::Unclear };
        return ready(verdict, DecisionSupport::Unknown, answer, vec!["本次搜索没有完成整轮，暂不评价走法优劣".to_string()]);
    }
    let mut points = Vec::new();
    // 对照着法：棋谱中的着法；引擎参考在同一次搜索中明显好于它时改用引擎参考
    let mut reference = answer;
    if let Some(b) = best.filter(|b| *b != answer)
        && let (Some(lb), Some(la)) = (line(b), line(answer))
        && let (CompareOutcome::MineBetter, text) = judge_pair(lb, la, &name(before, b), &name(before, answer))
    {
        reference = b;
        points.push(format!("本次搜索中，{text}：棋谱中的这一着不一定是最好的参考，下面以引擎参考{}对照", name(before, b)));
    }
    if choice == answer {
        // 棋谱着法明显不如引擎参考时，与棋谱一致不等于选对
        let support = if reference == answer { DecisionSupport::Supported } else { DecisionSupport::NotSupported };
        return ready(DecisionVerdict::SameAsRecord, support, reference, points);
    }
    if choice == reference {
        // 这次的选择就是本次搜索中明显好于棋谱着法的引擎参考
        return ready(DecisionVerdict::Reasonable, DecisionSupport::Supported, reference, points);
    }
    let (Some(lc), Some(lr)) = (line(choice), line(reference)) else {
        points.push("搜索没有同时给出这两步的结果，暂不评价走法优劣".to_string());
        return ready(DecisionVerdict::Unclear, DecisionSupport::Unknown, reference, points);
    };
    let (choice_name, reference_name) = (name(before, choice), name(before, reference));
    let (outcome, text) = judge_pair(lc, lr, &choice_name, &reference_name);
    points.push(format!("同一次搜索中，{text}"));
    let (verdict, support) = match outcome {
        CompareOutcome::MineBetter | CompareOutcome::Close => (DecisionVerdict::Reasonable, DecisionSupport::Supported),
        CompareOutcome::ReferenceBetter if exact_gap(lc, lr).is_some_and(|d| d <= NOTEWORTHY_GAP) => {
            (DecisionVerdict::Reasonable, DecisionSupport::Supported)
        }
        CompareOutcome::ReferenceBetter => {
            if let Some(reply) = lc.pv.get(1) {
                points.push(format!("走{choice_name}之后，对方最值得关注的回应是{}", name(&before.after(choice), *reply)));
            }
            (DecisionVerdict::Noteworthy, DecisionSupport::NotSupported)
        }
        CompareOutcome::Unclear | CompareOutcome::NotAnalyzed => (DecisionVerdict::Unclear, DecisionSupport::Unknown),
    };
    ready(verdict, support, reference, points)
}

impl Inner {
    /// 卡片局面的搜索条件：原会话中的这一着仍在时与讲解使用同样的历史（可以复用已有的分析），
    /// 否则从卡片保存的局面开始（没有此前的历史）。
    fn card_conditions(&self, card: &ReviewCard, searchmoves: Vec<Move>) -> ApiResult<AnalysisConditions> {
        let source = self.store.load_study(&card.study_id).ok().flatten().and_then(|s| {
            let node = s.nodes.get(&card.node_id)?;
            (node.mv == Some(card.answer.mv)).then_some(())?;
            s.engine_history(node.parent?).ok()
        });
        let (base_fen, moves) = source.unwrap_or_else(|| (card.fen.clone(), Vec::new()));
        self.conditions_from(base_fen, moves, Budget::Quick, searchmoves)
    }

    /// 开始比较一次复习的选择（局面决策）。结果写回这次复习（按时间找到它）后推送。
    pub(crate) fn review_decide(&self, card: &ReviewCard, at: i64, choice: Move) {
        let fail = |inner: &Inner, e: String| inner.finish_decision(&card.id, at, DecisionFeedback::failed(e));
        let pos = match fen::parse_position(&card.fen) {
            Ok(p) => p,
            Err(e) => return fail(self, format!("卡片中的局面无法读取：{e}")),
        };
        let first = match self.card_conditions(card, Vec::new()) {
            Ok(c) => c,
            Err(e) => return fail(self, e.message),
        };
        let (weak, card2) = (self.weak.clone(), card.clone());
        let group = format!("review:{}", card.id);
        let group2 = group.clone();
        self.search(first, Priority::Interactive, group, None, move |outcome| {
            let Some(inner) = weak.upgrade() else { return };
            let fail = |e: String| inner.finish_decision(&card2.id, at, DecisionFeedback::failed(e));
            let best = match outcome {
                SearchOutcome::Completed(r) => r.lines.first().map(|l| l.mv).filter(|mv| rules::is_legal(&pos, *mv)),
                SearchOutcome::Terminal(_) => return fail("这个局面已经结束，不能比较".to_string()),
                SearchOutcome::Failed(e) => return fail(e),
                SearchOutcome::Cancelled => return fail("搜索已取消".to_string()),
                // 设置在排队期间改变：按新设置重新比较
                SearchOutcome::Stale(_) => return inner.review_decide(&card2, at, choice),
            };
            let answer = card2.answer.mv;
            let mut moves = vec![choice, answer];
            moves.extend(best);
            moves.sort_by_key(|m| m.iccs());
            moves.dedup();
            let second = match inner.card_conditions(&card2, moves) {
                Ok(c) => c,
                Err(e) => return fail(e.message),
            };
            let conditions = second.describe();
            let weak = inner.weak.clone();
            let card3 = card2.clone();
            inner.search(second, Priority::Interactive, group2, None, move |outcome| {
                let Some(inner) = weak.upgrade() else { return };
                let feedback = match outcome {
                    SearchOutcome::Completed(r) => DecisionFeedback {
                        conditions: Some(conditions),
                        ..judge_decision(&pos, choice, answer, best, &r.lines, r.complete)
                    },
                    SearchOutcome::Terminal(_) => DecisionFeedback::failed("这个局面已经结束，不能比较".to_string()),
                    SearchOutcome::Failed(e) => DecisionFeedback::failed(e),
                    SearchOutcome::Cancelled => DecisionFeedback::failed("搜索已取消".to_string()),
                    SearchOutcome::Stale(_) => return inner.review_decide(&card3, at, choice),
                };
                inner.finish_decision(&card3.id, at, feedback);
            });
        });
    }

    /// 写回这次复习的比较结果并推送卡片。卡片已经删除、或这次复习已被更早的记录挤出时不再写入。
    ///
    /// 比较完成前已经按这次复习标记过状态（安排先按“还没有结果”计算）：如果这仍是最近一次复习，
    /// 按结果从同一个起点重新安排；之后又有了新的复习时不改动，留给新的复习安排。
    fn finish_decision(&self, id: &str, at: i64, feedback: DecisionFeedback) {
        let intervals = self.config_snapshot().review.intervals_days;
        let updated = self.store.update_review(id, |c| {
            if let Some(a) = c.attempts.iter_mut().find(|a| a.at == at) {
                a.decision = Some(feedback);
            }
            let latest = c.attempts.last().is_some_and(|a| a.at == at);
            let marked = c.schedule_base.is_some_and(|b| b.attempt_at == Some(at));
            if latest && marked && c.status != ReviewStatus::Due {
                schedule::apply(c, c.status, now_ms(), &intervals);
                c.updated_at = now_ms();
            }
        });
        match updated {
            Ok(Some(card)) => events::emit(self.events.as_ref(), events::REVIEW_UPDATED, &self.review_item(card)),
            Ok(None) => {}
            Err(e) => warn!("保存复习的比较结果失败：{e}"),
        }
    }
}

/// 局面决策开始时的状态：搜索完成前为“进行中”。
pub(crate) fn pending() -> DecisionFeedback { DecisionFeedback::pending() }

#[cfg(test)]
mod tests {
    use xq_core::fen::parse_position;
    use xq_core::uci::Bound;
    use xq_core::uci::Score;

    use super::*;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    fn line(m: &str, cp: i32, pv: &[&str]) -> EngineLine {
        EngineLine { mv: mv(m), score: Score::Cp(cp), bound: Bound::Exact, depth: 10, pv: pv.iter().map(|s| mv(s)).collect() }
    }

    #[test]
    fn a_different_but_close_choice_is_reasonable_and_a_clearly_worse_one_is_noteworthy() {
        let start = Position::startpos();
        // 棋谱：炮二平五；这次的选择：马二进三（接近）或兵三进一（明显低）
        let lines = [line("h2e2", 30, &["h2e2", "h9g7"]), line("h0g2", 20, &["h0g2", "b9c7"]), line("g3g4", -60, &["g3g4", "h7e7"])];
        let same = judge_decision(&start, mv("h2e2"), mv("h2e2"), None, &lines, true);
        assert_eq!((same.verdict, same.support), (Some(DecisionVerdict::SameAsRecord), DecisionSupport::Supported));
        let close = judge_decision(&start, mv("h0g2"), mv("h2e2"), None, &lines, true);
        assert_eq!(close.verdict, Some(DecisionVerdict::Reasonable));
        assert!(close.points[0].contains("评分接近"), "{:?}", close.points);
        // 略逊（不超过 80 分）仍算接近，说明中写明分差
        let slight = [line("h2e2", 30, &["h2e2"]), line("b0c2", -40, &["b0c2"])];
        let slightly = judge_decision(&start, mv("b0c2"), mv("h2e2"), None, &slight, true);
        assert_eq!(slightly.verdict, Some(DecisionVerdict::Reasonable));
        assert!(slightly.points[0].contains("低约 70 分（略逊）"), "{:?}", slightly.points);
        let worse = judge_decision(&start, mv("g3g4"), mv("h2e2"), None, &lines, true);
        assert_eq!((worse.verdict, worse.support), (Some(DecisionVerdict::Noteworthy), DecisionSupport::NotSupported));
        assert!(worse.points[0].contains("兵三进一比炮二平五低约 90 分"), "{:?}", worse.points);
        assert_eq!(worse.points[1], "走兵三进一之后，对方最值得关注的回应是炮8平5");
        assert_eq!(worse.reference.unwrap().notation, "炮二平五");
    }

    #[test]
    fn a_clearly_better_engine_move_replaces_the_record_as_the_reference() {
        let start = Position::startpos();
        // 棋谱：兵三进一（明显低于引擎参考炮二平五）
        let lines = [line("h2e2", 30, &["h2e2", "h9g7"]), line("g3g4", -60, &["g3g4"]), line("h0g2", 25, &["h0g2"])];
        let close_to_best = judge_decision(&start, mv("h0g2"), mv("g3g4"), Some(mv("h2e2")), &lines, true);
        assert_eq!(close_to_best.reference.as_ref().unwrap().notation, "炮二平五");
        assert!(close_to_best.points[0].contains("不一定是最好的参考"), "{:?}", close_to_best.points);
        assert_eq!(close_to_best.verdict, Some(DecisionVerdict::Reasonable));
        // 走了引擎参考本身
        assert_eq!(judge_decision(&start, mv("h2e2"), mv("g3g4"), Some(mv("h2e2")), &lines, true).verdict, Some(DecisionVerdict::Reasonable));
        // 与棋谱一致，但棋谱着法明显不如引擎参考：仍陈述“与棋谱一致”，但这次比较不支持这个选择
        let same = judge_decision(&start, mv("g3g4"), mv("g3g4"), Some(mv("h2e2")), &lines, true);
        assert_eq!((same.verdict, same.support), (Some(DecisionVerdict::SameAsRecord), DecisionSupport::NotSupported));
        assert!(same.points[0].contains("不一定是最好的参考"));
        // 早期的记录没有这一项：按结论与对照着法推算
        let legacy = DecisionFeedback { support: DecisionSupport::Legacy, ..same.clone() };
        assert_eq!(legacy.support_for(Some(mv("g3g4"))), DecisionSupport::NotSupported);
        let legacy_ok = DecisionFeedback { support: DecisionSupport::Legacy, ..close_to_best.clone() };
        assert_eq!(legacy_ok.support_for(Some(mv("h0g2"))), DecisionSupport::Supported);
        assert_eq!(DecisionFeedback::pending().support_for(Some(mv("h0g2"))), DecisionSupport::Unknown);
    }

    #[test]
    fn incomplete_or_bounded_searches_do_not_judge() {
        let start = Position::startpos();
        let lines = [line("h2e2", 30, &["h2e2"]), line("g3g4", -60, &["g3g4"])];
        let partial = judge_decision(&start, mv("g3g4"), mv("h2e2"), None, &lines, false);
        assert_eq!((partial.verdict, partial.support), (Some(DecisionVerdict::Unclear), DecisionSupport::Unknown));
        assert!(partial.points[0].contains("没有完成整轮"));
        // 搜索不完整时走了棋谱着法：陈述一致，但棋谱着法的好坏也没有得到比较
        let partial_same = judge_decision(&start, mv("h2e2"), mv("h2e2"), None, &lines, false);
        assert_eq!((partial_same.verdict, partial_same.support), (Some(DecisionVerdict::SameAsRecord), DecisionSupport::Unknown));
        let mut bounded = lines.clone();
        bounded[0].bound = Bound::Lower;
        bounded[1].bound = Bound::Lower;
        assert_eq!(judge_decision(&start, mv("g3g4"), mv("h2e2"), None, &bounded, true).verdict, Some(DecisionVerdict::Unclear));
        // 搜索没有给出这次选择的结果
        assert_eq!(judge_decision(&start, mv("b0c2"), mv("h2e2"), None, &lines, true).verdict, Some(DecisionVerdict::Unclear));
    }

    #[test]
    fn intents_are_checked_against_rule_facts_only() {
        let ctx = ExplainContext { history_complete: true, repetition_count: 1 };
        let start = Position::startpos();
        let r = reflect(&start, mv("h0g2"), Some(Intent::Develop), &ctx);
        assert_eq!(r.checks, vec!["这一步把右马从底线走出"]);
        assert!(!r.changes.is_empty(), "也列出这一步实际改变了什么");
        assert_eq!(reflect(&start, mv("h0g2"), Some(Intent::Capture), &ctx).checks, vec!["这一步没有吃子"]);
        assert_eq!(reflect(&start, mv("h0g2"), Some(Intent::Check), &ctx).checks, vec!["这一步没有将军"]);
        assert_eq!(reflect(&start, mv("h0g2"), Some(Intent::Defend), &ctx).checks, vec!["走之前没有己方棋子直接受到威胁", "这一步保护了仕和中兵"]);
        assert!(reflect(&start, mv("h0g2"), None, &ctx).checks.is_empty());
        assert!(reflect(&start, mv("h0g2"), Some(Intent::Other), &ctx).checks.is_empty());
        // 炮二进七打马
        assert_eq!(reflect(&start, mv("h2h9"), Some(Intent::Capture), &ctx).checks, vec!["这一步吃掉了对方的马"]);
        // 受到车攻击、没有保护的炮走开；仍留在车的线路上则仍受威胁
        let attacked = parse_position("r3k4/9/9/9/C8/9/9/9/9/3K5 w").unwrap();
        let away = reflect(&attacked, mv("a5b5"), Some(Intent::Defend), &ctx).checks;
        assert!(away.len() == 1 && away[0].contains("原本受到攻击") && away[0].contains("现在不再受到这个威胁"), "{away:?}");
        let stays = reflect(&attacked, mv("a5a4"), Some(Intent::Defend), &ctx).checks;
        assert!(stays.iter().any(|c| c.contains("仍然受到攻击")), "{stays:?}");
        // 有保护、但受到价值更低的棋子攻击（卒攻击车，马能吃回）：同样是威胁，走开后解除
        let pawn = parse_position("4k4/9/9/9/p3P4/R8/2N6/9/9/4K4 w").unwrap();
        let moved = reflect(&pawn, mv("a4b4"), Some(Intent::Defend), &ctx).checks;
        assert!(!moved.iter().any(|c| c.contains("没有己方棋子直接受到威胁")), "{moved:?}");
        assert!(
            moved.iter().any(|c| c.contains("车原本受到价值更低的棋子攻击（能吃回，但交换吃亏），现在不再受到这个威胁")),
            "{moved:?}"
        );
        // 车不动（走帅）：仍受威胁；马走开后车失去保护，威胁变为缺少保护
        let other = reflect(&pawn, mv("e0e1"), Some(Intent::Defend), &ctx).checks;
        assert!(other.iter().any(|c| c.contains("车仍然受到价值更低的棋子攻击")), "{other:?}");
        let unguarded = reflect(&pawn, mv("c3d5"), Some(Intent::Defend), &ctx).checks;
        assert!(unguarded.iter().any(|c| c.contains("车仍然受到攻击、缺少保护")), "{unguarded:?}");
    }
}
