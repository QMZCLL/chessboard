//! 整局报告：从一盘棋的主线中选出最值得复盘的几处局面，并整理值得保留的判断与需要重新观察的地方。
//!
//! - 筛选与统计都由程序完成；文字取自已经生成的讲解（一句话结论、当前需求、对方的应对），不调用模型。
//! - 评价只用每一着自己的同一次搜索：讲解中实际着法与首选的比较。不把不同局面、不同分析条件、
//!   不同视角的评分相减成曲线；分析不完整的着法不按评价入选，只按练习记录、收藏与规则事实入选。
//! - 入选的依据：实际着法明显不如首选；先想后讲中自己的选择不如首选、与棋谱不同、没有走子就看讲解、
//!   用了较多提示；收藏；吃子与将军。相邻几着中同一类依据的只保留一处，最多五处。
//! - “需要重新观察的地方”只说明“这条关系值得重新检查”，不替用户认定当时错在哪里。

use std::collections::HashMap;

use serde::Serialize;
use xq_core::explain::ExplainLevel;
use xq_core::explain::MoveExplanation;
use xq_core::explain::Verdict;
use xq_core::explain::VerdictTone;
use xq_core::explain::facts;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::review::ReviewStatus;
use crate::review::turn_of;
use crate::service::ApiResult;
use crate::service::AppService;

/// 最多选出的关键局面数。
const MAX_KEY: usize = 5;
/// 入选的最低分值。
const MIN_WEIGHT: u32 = 2;
/// 相邻这么多着之内、依据相同的局面只保留一处。
const NEAR_PLIES: u32 = 2;

/// 入选的依据类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyReason {
    /// 实际着法不如首选。
    Mistake,
    /// 先想后讲中自己的选择与棋谱不同、不如首选，或没有走子就看了讲解。
    Choice,
    /// 先想后讲中用了较多提示。
    Hints,
    Starred,
    /// 吃子、将军等战术关系。
    Tactic,
}

#[derive(Clone, Debug, Serialize)]
pub struct KeyPosition {
    pub node_id: NodeId,
    pub ply: u32,
    /// 如“第 3 回合 · 红方 马二进三”。
    pub label: String,
    /// 入选的依据（程序给出的说明）。
    pub reasons: Vec<String>,
    /// 讲解中的一句话结论；还没有讲解时为空。
    pub summary: String,
    /// 是否已在复习中。
    pub review: Option<ReviewStatus>,
    #[serde(skip)]
    weight: u32,
    /// 分值最高的依据类别，用于合并相邻的同类局面。
    #[serde(skip)]
    main: KeyReason,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportItem {
    pub node_id: NodeId,
    pub label: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PracticeStats {
    /// 先想后讲练习过的着法数。
    pub practiced: usize,
    /// 第一步与棋谱一致。
    pub matched: usize,
    /// 用了提示。
    pub hinted: usize,
    /// 没有走子就看了讲解。
    pub revealed: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct GameReport {
    pub study_id: String,
    pub study_name: String,
    /// 主线的着法数与其中有完整引擎分析的着法数。
    pub moves: usize,
    pub detailed: usize,
    pub practice: PracticeStats,
    /// 本局关键局面（按着法顺序）。
    pub key_positions: Vec<KeyPosition>,
    /// 值得保留的判断：先想后讲中选对或与首选接近。
    pub good_choices: Vec<ReportItem>,
    /// 需要重新观察的地方。
    pub to_recheck: Vec<ReportItem>,
    pub notes: Vec<String>,
}

/// 完整分析的讲解中的评价。
fn verdict(e: Option<&MoveExplanation>) -> Option<&Verdict> { e.filter(|e| e.level == ExplainLevel::Detailed)?.verdict.as_ref() }

fn verdict_text(v: &Verdict) -> String {
    match v.delta {
        Some(d) => format!("{}（比首选低约 {d} 分）", v.label),
        None => v.label.clone(),
    }
}

/// 按分值选出关键局面：相邻几着中主要依据相同的只保留分值高的一处，最多五处，再按着法顺序排列。
pub fn pick(mut candidates: Vec<KeyPosition>) -> Vec<KeyPosition> {
    candidates.retain(|c| c.weight >= MIN_WEIGHT);
    candidates.sort_by(|a, b| b.weight.cmp(&a.weight).then(a.ply.cmp(&b.ply)));
    let mut picked: Vec<KeyPosition> = Vec::new();
    for c in candidates {
        if picked.len() >= MAX_KEY {
            break;
        }
        if picked.iter().any(|p| p.main == c.main && p.ply.abs_diff(c.ply) <= NEAR_PLIES) {
            continue;
        }
        picked.push(c);
    }
    picked.sort_by_key(|p| p.ply);
    picked
}

impl AppService {
    /// 一盘棋的整局报告（见模块说明）。
    pub fn game_report(&self, id: &str) -> ApiResult<GameReport> {
        let inner = &self.inner;
        let study: Study = inner.with_study(id, |s| Ok(s.clone()))?;
        let explanations: HashMap<NodeId, MoveExplanation> = inner
            .store
            .explanations_for(id)?
            .into_iter()
            .filter_map(|r| Some((r.node_id, r.explanation?)))
            .collect();
        let root_side = study.node(xq_core::study::ROOT)?.side_to_move;
        let mut practice = PracticeStats::default();
        let mut candidates = Vec::new();
        let mut good_choices = Vec::new();
        let (mut moves, mut detailed) = (0, 0);
        for node_id in study.mainline() {
            let node = study.node(node_id)?;
            let (Some(mv), Some(parent)) = (node.mv, node.parent) else { continue };
            moves += 1;
            let expl = explanations.get(&node_id);
            if expl.is_some_and(|e| e.level == ExplainLevel::Detailed) {
                detailed += 1;
            }
            let notation = node.notation.clone().unwrap_or_else(|| mv.iccs());
            let mover = study.node(parent)?.side_to_move;
            let label = format!("第 {} 回合 · {} {notation}", turn_of(node.ply, root_side), mover.label());
            let mut weights: Vec<(KeyReason, u32, String)> = Vec::new();

            if let Some(v) = verdict(expl) {
                let w = match v.tone {
                    VerdictTone::Blunder => 6,
                    VerdictTone::Mistake => 4,
                    VerdictTone::Inaccuracy => 1,
                    _ => 0,
                };
                if w > 0 {
                    weights.push((KeyReason::Mistake, w, format!("实际着法{}", verdict_text(v))));
                }
            }
            if let Some(p) = &node.practice {
                practice.practiced += 1;
                practice.matched += usize::from(p.matched);
                practice.hinted += usize::from(p.hints_used > 0);
                practice.revealed += usize::from(p.revealed_directly);
                // 自己走出的选择在棋谱中是这一着的兄弟分支，有自己的讲解与评价
                let attempt = p.first_choice.filter(|_| !p.matched).and_then(|choice| {
                    let sibling = study.node(parent).ok()?.children.iter().copied().find(|c| study.nodes.get(c).is_some_and(|n| n.mv == Some(choice)))?;
                    Some((study.nodes.get(&sibling)?.notation.clone().unwrap_or_else(|| choice.iccs()), verdict(explanations.get(&sibling))))
                });
                if p.revealed_directly {
                    weights.push((KeyReason::Choice, 1, "先想后讲：没有走子就看了讲解".into()));
                } else if p.matched {
                    good_choices.push(ReportItem { node_id, label: label.clone(), text: format!("先想后讲中第一步与棋谱一致：{notation}") });
                } else if let Some((choice, v)) = attempt {
                    match v {
                        Some(v) if matches!(v.tone, VerdictTone::Mistake | VerdictTone::Blunder) => {
                            weights.push((KeyReason::Choice, 4, format!("先想后讲：你的选择{choice}{}", verdict_text(v))));
                        }
                        Some(v) if matches!(v.tone, VerdictTone::Best | VerdictTone::Close) => {
                            good_choices.push(ReportItem {
                                node_id,
                                label: label.clone(),
                                text: format!("先想后讲中你的选择{choice}与首选接近（棋谱中是{notation}）"),
                            });
                        }
                        _ => weights.push((KeyReason::Choice, 1, format!("先想后讲：第一步走了{choice}，与棋谱不同"))),
                    }
                }
                if p.hints_used >= 2 {
                    weights.push((KeyReason::Hints, 2, format!("先想后讲：用了 {} 级提示", p.hints_used)));
                }
            }
            if node.starred {
                weights.push((KeyReason::Starred, 2, "收藏".into()));
            }
            if let Some(f) = study.position(parent).ok().and_then(|pos| facts::move_facts(&pos, mv)) {
                if let Some(p) = f.captured {
                    weights.push((KeyReason::Tactic, 1, format!("吃掉对方的{}", p.name())));
                }
                if !f.checkers.is_empty() {
                    weights.push((KeyReason::Tactic, 1, "将军".into()));
                }
            }
            let weight = weights.iter().map(|(_, w, _)| w).sum();
            let Some(main) = weights.iter().max_by_key(|(_, w, _)| *w).map(|(k, _, _)| *k) else { continue };
            candidates.push(KeyPosition {
                node_id,
                ply: node.ply,
                label,
                reasons: weights.into_iter().map(|(_, _, t)| t).collect(),
                summary: expl.map(|e| e.summary.clone()).unwrap_or_default(),
                review: inner.review_status(id, node_id),
                weight,
                main,
            });
        }
        let key_positions = pick(candidates);
        let to_recheck = key_positions
            .iter()
            .filter(|k| matches!(k.main, KeyReason::Mistake | KeyReason::Choice))
            .filter_map(|k| {
                let e = explanations.get(&k.node_id)?;
                let fact = e.need.first().or(e.replies.first()).map(|c| c.text.clone()).unwrap_or_else(|| e.summary.clone());
                Some(ReportItem { node_id: k.node_id, label: k.label.clone(), text: format!("这条关系值得重新检查：{fact}") })
            })
            .collect();
        good_choices.truncate(MAX_KEY);

        let mut notes = vec!["评价只比较每一着自己的候选（同一次搜索、同一视角），不同局面之间的评分不直接相减。".to_string()];
        if detailed < moves {
            notes.push(format!("主线 {moves} 着中 {detailed} 着有完整的引擎分析；其余着法只按练习记录、收藏与规则事实入选。"));
        }
        if key_positions.len() < 3 {
            notes.push(format!("这盘棋中达到条件的关键局面只有 {} 处。", key_positions.len()));
        }
        Ok(GameReport {
            study_id: study.id.clone(),
            study_name: study.name.clone(),
            moves,
            detailed,
            practice,
            key_positions,
            good_choices,
            to_recheck,
            notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(ply: u32, weight: u32, main: KeyReason) -> KeyPosition {
        KeyPosition { node_id: ply, ply, label: String::new(), reasons: vec![], summary: String::new(), review: None, weight, main }
    }

    #[test]
    fn picks_at_most_five_by_weight_and_merges_nearby_positions_of_the_same_kind() {
        use KeyReason::*;
        let picked = pick(vec![
            candidate(3, 6, Mistake),
            // 与第 3 着相邻、同为评价问题：只保留分值高的一处
            candidate(5, 4, Mistake),
            // 相邻但依据不同：保留
            candidate(4, 2, Starred),
            // 分值不够
            candidate(8, 1, Tactic),
            candidate(10, 4, Choice),
            candidate(12, 2, Hints),
            candidate(20, 4, Mistake),
            candidate(30, 2, Starred),
        ]);
        // 最多五处；分值相同时先出现的优先（第 30 着落选）；结果按着法顺序
        assert_eq!(picked.iter().map(|p| p.ply).collect::<Vec<_>>(), [3, 4, 10, 12, 20]);
        assert!(pick(vec![candidate(1, 1, Tactic)]).is_empty());
    }
}
