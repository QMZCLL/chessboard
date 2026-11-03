//! 从自己的棋局生成练习。只用已经完成的讲解与规则事实，不另外搜索；不满足门槛的局面不出“唯一答案”的题目，
//! 并说明跳过的原因。
//!
//! - 只出自己一方（会话的“我执”一方）在主线上走的着法；缺口之后的局面来自不确定的识别，不出题。
//! - 找威胁：落子前己方有没有保护、会被吃掉的棋子，或正被将军；这一步的评价不好，或先想后讲中没有选对时才出，
//!   答案是这些棋子（与将军的棋子）所在的格子，按规则事实判断。
//! - 二选一、找更好的着法：当时的着法与引擎首选在同一次搜索中（实际着法在候选中），搜索高稳定，
//!   程序评价为“明显不如首选”或“严重失误”。二选一给出这两步；找更好的着法在局面中自己选择（局面决策）。
//! - 同一局面同一题型已经有卡片时不重复加入（重复点“生成练习”不会多出题目）。
//! - 一次生成的题目在同一个写事务中加入：中途出错时一张都不加入，不会留下半批题目。
//! - 题目的局面、答案、选项与出处在生成时保存在卡片中：原会话之后被修改或删除、引擎或设置改变，都不改变已有题目的答案；
//!   出处（`ExerciseProvenance`）写明出题规则的版本、这一次出题、原会话的棋谱版本与依据的分析条件。

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;
use xq_core::explain::VerdictTone;
use xq_core::explain::facts;
use xq_core::explain::stability::StabilityLevel;
use xq_core::notation;
use xq_core::study::NodeId;
use xq_core::study::ROOT;
use xq_core::study::Study;
use xq_core::Square;

use crate::review::CardKind;
use crate::review::ReviewCard;
use crate::review::ReviewMove;
use crate::review::kind_card_id;
use crate::service::ApiResult;
use crate::service::Inner;
use crate::storage::Inserted;

/// 一次最多加入的找威胁题。
const MAX_THREATS: usize = 5;

/// 出题规则的版本：出题的门槛或题目的内容改变时加一。
pub const GENERATOR_VERSION: u32 = 1;

/// 生成的题目的出处。题目的内容在生成时已经保存在卡片中，这些信息说明它从哪里来，以后可以据此判断是否需要重新出题。
/// 出题没有随机成分（二选一的选项按坐标排列），同样的讲解总是出同样的题。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExerciseProvenance {
    /// 出题规则的版本（见 `GENERATOR_VERSION`）。
    pub generator_version: u32,
    /// 这一次出题的编号：同一次加入的题目相同。
    pub generation_id: String,
    pub generated_at: i64,
    /// 出题时原会话的棋谱版本。
    pub source_revision: u64,
    /// 依据的讲解的规则版本；没有讲解时为空。
    pub explain_version: Option<u32>,
    /// 依据的分析条件（含引擎与网络文件的内容校验值）；没有引擎分析时为空。
    pub conditions: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExerciseRequest {
    pub study_id: String,
    /// 重新出题：按现在的分析再出一次（新的出题编号），还没有作答过、内容又变了的题目换成新的内容；作答过的题目保留。
    #[serde(default)]
    pub regenerate: bool,
}

/// 生成的结果。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ExerciseSummary {
    pub threat: u32,
    pub choice: u32,
    pub better: u32,
    /// 已经有这道题，没有重复加入（重新出题时：作答过或内容没有变的题目）。
    pub existing: u32,
    /// 重新出题时换成新内容的题目（之前没有作答过）。
    pub updated: u32,
    /// 没有出题的局面与原因（最多列出 5 处）。
    pub skipped: Vec<String>,
    /// 一句话说明。
    pub text: String,
}

/// 落子前这一方受到威胁的棋子（没有保护、会被吃掉）与将军的棋子所在的格子。
pub fn threat_targets(fen: &str) -> Vec<Square> {
    let Ok(pos) = xq_core::fen::parse_position(fen) else { return Vec::new() };
    let f = facts::position_facts(&pos);
    let mut out: Vec<Square> = f.own_threats.iter().filter(|t| !t.defended).map(|t| t.square).collect();
    out.extend(f.checkers.iter().copied());
    out.sort();
    out.dedup();
    out
}

impl Inner {
    pub(crate) fn generate_exercises(&self, study_id: &str, regenerate: bool) -> ApiResult<ExerciseSummary> {
        let study: Study = self.with_study(study_id, |s| Ok(s.clone()))?;
        let mut summary = ExerciseSummary::default();
        let mut skipped = Vec::new();
        let mut threats = 0;
        let generation = (uuid::Uuid::new_v4().to_string(), crate::service::now_ms());
        // 先整理出全部题目，最后在同一个写事务中加入
        let mut cards: Vec<ReviewCard> = Vec::new();
        for node_id in study.mainline().into_iter().filter(|n| *n != ROOT) {
            let node = study.node(node_id)?;
            let (Some(parent), Some(played)) = (node.parent, node.mv) else { continue };
            let before = study.node(parent)?;
            if before.side_to_move != study.user_side || study.segment_start(parent)? != ROOT {
                continue;
            }
            let round = |ply: u32| format!("第 {} 回合", crate::review::turn_of(ply, study.node(ROOT).map(|r| r.side_to_move).unwrap_or(study.user_side)));
            let record = self.store.load_explanation(&study.id, node_id)?;
            let expl = record.as_ref().and_then(|r| r.explanation.as_ref());
            let provenance = ExerciseProvenance {
                generator_version: GENERATOR_VERSION,
                generation_id: generation.0.clone(),
                generated_at: generation.1,
                source_revision: study.revision,
                explain_version: expl.map(|e| e.generator_version),
                conditions: record.as_ref().and_then(|r| r.conditions_key.clone()),
            };
            let tone = expl.and_then(|e| e.verdict.as_ref()).map(|v| v.tone);
            let poor = matches!(tone, Some(VerdictTone::Inaccuracy | VerdictTone::Mistake | VerdictTone::Blunder));
            let missed_in_think = node.practice.as_ref().is_some_and(|p| !p.matched);

            // 找威胁
            let targets = threat_targets(&before.fen);
            if !targets.is_empty() && (poor || missed_in_think) && threats < MAX_THREATS {
                let why = if poor { "这一步的评价不好" } else { "先想后讲中没有选对" };
                let origin = format!("{}：走这一步之前有 {} 处受到威胁，{why}", round(node.ply), targets.len());
                cards.push(self.exercise_card(&study, node_id, CardKind::Threat, origin, &provenance, |c| c.targets = targets.clone())?);
                threats += 1;
            }

            // 二选一与找更好的着法
            let Some(expl) = expl.filter(|_| matches!(tone, Some(VerdictTone::Mistake | VerdictTone::Blunder))) else { continue };
            let Some(best) = expl.candidates.iter().find(|c| c.rank == Some(1)) else { continue };
            let same_search = expl.candidates.iter().any(|c| c.mv == played && c.rank.is_some());
            let stable = expl.stability.as_ref().is_some_and(|s| s.level == StabilityLevel::High);
            if !same_search || !stable || best.mv == played {
                let reason = if !same_search { "当时的着法不在同一次搜索的候选中" } else { "搜索结果还不够稳定" };
                skipped.push(format!("{}{}：{reason}", round(node.ply), expl.notation));
                continue;
            }
            let pos = study.position(parent)?;
            let name = |mv: xq_core::Move| notation::move_to_chinese(&pos, mv).unwrap_or_else(|_| mv.iccs());
            let best_move = ReviewMove { mv: best.mv, notation: name(best.mv) };
            let played_move = ReviewMove { mv: played, notation: expl.notation.clone() };
            let label = expl.verdict.as_ref().map(|v| v.label.clone()).unwrap_or_default();
            let origin = format!("{}：当时走了{}，程序评价为“{label}”（同一次搜索，高稳定）", round(node.ply), expl.notation);
            for kind in [CardKind::Choice, CardKind::Better] {
                let (best_move, played_move) = (best_move.clone(), played_move.clone());
                cards.push(self.exercise_card(&study, node_id, kind, origin.clone(), &provenance, |c| {
                    c.answer = best_move.clone();
                    c.played = Some(played_move.clone());
                    if kind == CardKind::Choice {
                        let mut options = vec![best_move, played_move];
                        options.sort_by_key(|m| m.mv.iccs());
                        c.options = options;
                    }
                })?);
            }
        }
        // 同一局面同一题型已经有卡片时不加入；检查与写入在同一个写事务中
        let outcome = self.store.insert_reviews(&cards, regenerate)?;
        for (card, result) in cards.iter().zip(outcome) {
            match (result, card.kind) {
                (Inserted::Kept, _) => summary.existing += 1,
                (Inserted::Replaced, _) => summary.updated += 1,
                (Inserted::Added, CardKind::Threat) => summary.threat += 1,
                (Inserted::Added, CardKind::Choice) => summary.choice += 1,
                (Inserted::Added, _) => summary.better += 1,
            }
        }
        let added = summary.threat + summary.choice + summary.better;
        let mut text = if added == 0 {
            "没有新的题目".to_string()
        } else {
            format!("新增 {added} 道：找威胁 {}、二选一 {}、找更好的着法 {}", summary.threat, summary.choice, summary.better)
        };
        if summary.updated > 0 {
            text.push_str(&format!("；{} 道还没有作答过的题目按现在的分析更新", summary.updated));
        }
        if summary.existing > 0 {
            let kept = if regenerate { "道已经在复习中，作答过或内容没有变化，保留原来的题目" } else { "道已经在复习中" };
            text.push_str(&format!("；{} {kept}", summary.existing));
        }
        if !skipped.is_empty() {
            text.push_str(&format!("；{} 处失误没有出唯一答案的题目（{}）", skipped.len(), skipped.iter().take(2).cloned().collect::<Vec<_>>().join("；")));
        }
        text.push('。');
        summary.text = text;
        skipped.truncate(5);
        summary.skipped = skipped;
        Ok(summary)
    }

    /// 整理一道生成的题目（还没有加入）。
    fn exercise_card(
        &self, study: &Study, node_id: NodeId, kind: CardKind, origin: String, provenance: &ExerciseProvenance,
        fill: impl FnOnce(&mut ReviewCard),
    ) -> ApiResult<ReviewCard> {
        let mut card = self.build_card(study, node_id)?;
        card.id = kind_card_id(&study.id, node_id, kind);
        card.kind = kind;
        card.origin = Some(origin);
        card.provenance = Some(provenance.clone());
        fill(&mut card);
        Ok(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threat_targets_are_unprotected_pieces_and_checkers() {
        // 正被将军：中路的炮隔着马将军
        assert_eq!(threat_targets("3k5/9/4c4/9/9/9/9/4N4/9/4K4 w"), vec![Square::parse("e7").unwrap()]);
        // 起始局面没有威胁
        assert!(threat_targets(xq_core::fen::START_FEN).is_empty());
        // 炮二平五之后，黑方中卒受到攻击、没有保护
        let after = threat_targets("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR b");
        assert_eq!(after, vec![Square::parse("e6").unwrap()]);
    }
}
