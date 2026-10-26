//! 试走对比：同一局面下“我的选择”与“对照选择”的规则事实、对方应对，以及同一次搜索给出的评分比较。
//!
//! 对比的评分来自对两步棋的同一次搜索（`searchmoves` 两步、两条候选），与讲解使用相同的快速分析条件，
//! 结果按条件缓存。请求时先返回规则事实；需要搜索时，完成后通过 `comparison_updated` 推送完整结果。
//! 同一局面只保留最新的对比请求，切换对照时取消尚未完成的上一次搜索。
//!
//! 每次请求有递增的序号，返回的结果与随后推送的完整结果都带有它：同一组走法可能先后在不同的分析条件下比较，
//! “已经完成”不代表“属于最新的请求”，界面按序号判断新旧，只让同一次请求的完整结果代替它尚未完成的结果。

use serde::Deserialize;
use serde::Serialize;
use xq_core::Move;
use xq_core::Position;
use xq_core::explain::compare;
use xq_core::explain::compare::CompareEvidence;
use xq_core::explain::compare::MoveComparison;
use xq_core::notation;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::engine::Priority;
use crate::engine::SearchOutcome;
use crate::engine::SearchResult;
use crate::events;
use crate::records::AnalysisConditions;
use crate::records::AnalysisOutcome;
use crate::records::Budget;
use crate::service::ApiError;
use crate::service::ApiResult;
use crate::service::Inner;

#[derive(Clone, Debug, Deserialize)]
pub struct CompareRequest {
    pub study_id: String,
    /// 比较起点：两步棋都从这个节点的局面走出。
    pub node_id: NodeId,
    pub mine: Move,
    /// 对照的着法；为空时依次选用棋谱中的实际着法、引擎首选、棋谱中的其他分支。
    #[serde(default)]
    pub reference: Option<Move>,
}

/// 对照着法与棋谱、分析的关系。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// 棋谱在这个局面之后的着法（第一个后续着法）。
    Actual,
    /// 棋谱中的其他分支。
    Branch,
    /// 引擎候选。
    Candidate,
    /// 用户指定的其他着法。
    Other,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompareOption {
    pub mv: Move,
    pub notation: String,
    pub kind: ReferenceKind,
    /// 在已缓存的局面分析中的候选名次。
    pub rank: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompareView {
    /// 这次对比请求的序号（推送的完整结果沿用请求时的序号）。
    pub seq: u64,
    /// 搜索条件的键；引擎不可用、不能搜索时为空。
    pub conditions_key: Option<String>,
    pub study_id: String,
    pub node_id: NodeId,
    pub mine: Move,
    pub reference: Move,
    pub reference_kind: ReferenceKind,
    pub reference_rank: Option<u32>,
    /// 同一次搜索尚未完成，完成后推送完整结果。
    pub pending: bool,
    /// 没有得到搜索结果的原因（引擎不可用、搜索失败等）；规则事实仍然有效。
    pub error: Option<String>,
    pub comparison: MoveComparison,
    /// 可以选作对照的着法（不含“我的选择”）。
    pub options: Vec<CompareOption>,
}

/// 已经得到的一组对比，见 `Inner::cached_comparison`。
pub(crate) struct CachedComparison {
    pub study: Study,
    pub comparison: MoveComparison,
    pub reference_kind: ReferenceKind,
    /// 包含两步棋的同一次搜索；为否时引擎不可用，只有规则事实。
    pub searched: bool,
    /// 这次搜索完成了整轮（两条变化来自同一深度）。
    pub complete: bool,
}

impl Inner {
    pub(crate) fn compare_moves(&self, req: CompareRequest) -> ApiResult<CompareView> {
        let study = self.with_study(&req.study_id, |s| Ok(s.clone()))?;
        let pos = study.position(req.node_id)?;
        let ctx = self.context_at(&study, req.node_id)?;
        let options = self.compare_options(&study, req.node_id, &pos, req.mine);
        let reference = match req.reference {
            Some(r) => r,
            None => default_reference(&options).ok_or_else(|| ApiError::from("这个局面没有可以对照的着法，可以从候选中选择".to_string()))?,
        };
        let facts_only = compare::compare_moves(&pos, req.mine, reference, &ctx, None)
            .ok_or_else(|| ApiError::from("对比的两步必须是这个局面中不同的合法着法".to_string()))?;
        let (kind, rank) = options
            .iter()
            .find(|o| o.mv == reference)
            .map(|o| (o.kind, o.rank))
            .unwrap_or((ReferenceKind::Other, None));
        let cond = self.compare_conditions(&study, req.node_id, req.mine, reference);
        let base = CompareView {
            seq: self.compare_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1,
            conditions_key: cond.as_ref().ok().map(|c| c.key()),
            study_id: req.study_id.clone(),
            node_id: req.node_id,
            mine: req.mine,
            reference,
            reference_kind: kind,
            reference_rank: rank,
            pending: false,
            error: None,
            comparison: facts_only.clone(),
            options,
        };
        let view = |comparison: MoveComparison, pending: bool, error: Option<String>| CompareView {
            pending,
            error,
            comparison,
            ..base.clone()
        };

        let cond = match cond {
            Ok(c) => c,
            // 引擎不可用时只给出规则事实
            Err(e) => return Ok(view(facts_only, false, Some(e.message))),
        };
        let full = |result: SearchResult| {
            let engine = compare_evidence(result, &cond);
            compare::compare_moves(&pos, req.mine, reference, &ctx, Some(&engine))
        };
        if let Ok(Some(record)) = self.store.load_analysis(&cond.key())
            && record.reusable()
            && let AnalysisOutcome::Completed { result } = record.outcome
            && let Some(comparison) = full(result)
        {
            return Ok(view(comparison, false, None));
        }

        let group = format!("compare:{}:{}", req.study_id, req.node_id);
        // 同一局面只保留最新的对比请求
        self.engine.cancel_group(&group);
        let weak = self.weak.clone();
        let sink = self.events.clone();
        let pending = view(facts_only.clone(), true, None);
        let retry = CompareRequest { reference: Some(reference), ..req.clone() };
        let (pos2, ctx2, cond2) = (pos.clone(), ctx, cond.clone());
        let finished = pending.clone();
        self.search(cond, Priority::Interactive, group, None, move |outcome| {
            let (comparison, error) = match outcome {
                SearchOutcome::Completed(result) => {
                    let engine = compare_evidence(result, &cond2);
                    match compare::compare_moves(&pos2, retry.mine, reference, &ctx2, Some(&engine)) {
                        Some(c) => (c, None),
                        None => (facts_only, Some("对比失败".to_string())),
                    }
                }
                SearchOutcome::Terminal(_) => (facts_only, Some("这个局面已经结束，不能比较".to_string())),
                SearchOutcome::Failed(e) => (facts_only, Some(e)),
                // 被同一局面更新的对比请求取代
                SearchOutcome::Cancelled => return,
                SearchOutcome::Stale(_) => {
                    // 设置在排队期间改变：按新设置重新比较，结果由新的请求推送
                    if let Some(inner) = weak.upgrade() {
                        let _ = inner.compare_moves(retry);
                    }
                    return;
                }
            };
            let done = CompareView { pending: false, error, comparison, ..finished };
            events::emit(sink.as_ref(), events::COMPARISON_UPDATED, &done);
        });
        Ok(pending)
    }

    /// 对比搜索的条件：两步棋不分先后，条件相同即可复用。
    fn compare_conditions(&self, study: &Study, node: NodeId, mine: Move, reference: Move) -> ApiResult<AnalysisConditions> {
        let mut searchmoves = vec![mine, reference];
        searchmoves.sort_by_key(|m| m.iccs());
        self.conditions(study, node, Budget::Quick, searchmoves)
    }

    /// 已经得到的对比（教练追问的证据）：只读取已保存的搜索结果，不发起新的搜索。
    /// 引擎不可用时只有规则事实；引擎可用而这组对比还没有搜索结果时返回错误，等对比完成后再用。
    pub(crate) fn cached_comparison(&self, study_id: &str, node: NodeId, mine: Move, reference: Move) -> ApiResult<CachedComparison> {
        let study = self.with_study(study_id, |s| Ok(s.clone()))?;
        let pos = study.position(node)?;
        let ctx = self.context_at(&study, node)?;
        let facts = compare::compare_moves(&pos, mine, reference, &ctx, None)
            .ok_or_else(|| ApiError::from("对比的两步必须是这个局面中不同的合法着法".to_string()))?;
        let reference_kind = self
            .compare_options(&study, node, &pos, mine)
            .iter()
            .find(|o| o.mv == reference)
            .map(|o| o.kind)
            .unwrap_or(ReferenceKind::Other);
        let (comparison, searched, complete) = match self.compare_conditions(&study, node, mine, reference) {
            Err(_) => (facts, false, false),
            Ok(cond) => match self.store.load_analysis(&cond.key())? {
                // 与界面看到的对比一致：不完整的结果也采用，对比中已经注明暂不比较优劣
                Some(record) if record.is_current_format() => match record.outcome {
                    AnalysisOutcome::Completed { result } => {
                        let complete = result.complete;
                        let engine = compare_evidence(result, &cond);
                        let full = compare::compare_moves(&pos, mine, reference, &ctx, Some(&engine))
                            .ok_or_else(|| ApiError::from("对比失败".to_string()))?;
                        (full, true, complete)
                    }
                    AnalysisOutcome::Terminal { .. } => return Err("这个局面已经结束，不能比较".into()),
                },
                _ => return Err("对比搜索还没有完成，完成后再提问".into()),
            },
        };
        Ok(CachedComparison { study, comparison, reference_kind, searched, complete })
    }

    /// 可以选作对照的着法：棋谱中的后续着法与已缓存局面分析中的候选。
    fn compare_options(&self, study: &Study, node: NodeId, pos: &Position, mine: Move) -> Vec<CompareOption> {
        let mut options: Vec<CompareOption> = Vec::new();
        let candidates = self.cached_position_evidence(study, node).map(|e| e.candidates).unwrap_or_default();
        let rank_of = |mv: Move| candidates.iter().position(|l| l.mv == mv).map(|i| i as u32 + 1);
        let name = |mv: Move| notation::move_to_chinese(pos, mv).unwrap_or_else(|_| mv.iccs());
        if let Ok(n) = study.node(node) {
            for (i, child) in n.children.iter().enumerate() {
                let Some(mv) = study.node(*child).ok().and_then(|c| c.mv) else { continue };
                let kind = if i == 0 { ReferenceKind::Actual } else { ReferenceKind::Branch };
                options.push(CompareOption { mv, notation: name(mv), kind, rank: rank_of(mv) });
            }
        }
        for (i, line) in candidates.iter().enumerate() {
            if !options.iter().any(|o| o.mv == line.mv) {
                options.push(CompareOption { mv: line.mv, notation: name(line.mv), kind: ReferenceKind::Candidate, rank: Some(i as u32 + 1) });
            }
        }
        options.retain(|o| o.mv != mine);
        options
    }
}

/// 没有指定对照时：先用棋谱中的实际着法，其次是引擎首选，最后是棋谱中的其他分支。
fn default_reference(options: &[CompareOption]) -> Option<Move> {
    let pick = |kind: ReferenceKind| options.iter().filter(|o| o.kind == kind).min_by_key(|o| o.rank.unwrap_or(u32::MAX));
    pick(ReferenceKind::Actual).or_else(|| pick(ReferenceKind::Candidate)).or_else(|| pick(ReferenceKind::Branch)).map(|o| o.mv)
}

fn compare_evidence(result: SearchResult, cond: &AnalysisConditions) -> CompareEvidence {
    CompareEvidence { lines: result.lines, conditions: cond.describe(), complete: result.complete, notes: result.notes }
}
