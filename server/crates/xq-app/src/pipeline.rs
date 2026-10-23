//! 分析与讲解流水线。
//!
//! 讲解某一着分三步：先用规则事实生成基本讲解；再对落子前局面做多候选搜索；
//! 实际走法不在候选中时，用 searchmoves 在相同预算下单独分析。搜索结果按完整条件缓存。

use serde::Serialize;
use tracing::warn;
use xq_core::Move;
use xq_core::Position;
use xq_core::explain;
use xq_core::explain::CandidateView;
use xq_core::explain::EngineEvidence;
use xq_core::explain::EngineLine;
use xq_core::explain::ExplainContext;
use xq_core::explain::PositionExplanation;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::api::AnalyzeRequest;
use crate::api::ExplainStats;
use crate::engine::EngineIdentity;
use crate::engine::EngineState;
use crate::engine::JobSpec;
use crate::engine::Priority;
use crate::engine::SearchOutcome;
use crate::engine::SearchProgress;
use crate::engine::SearchRequest;
use crate::events;
use crate::records::AnalysisConditions;
use crate::records::AnalysisOutcome;
use crate::records::AnalysisRecord;
use crate::records::Budget;
use crate::records::ExplainStatus;
use crate::records::ExplanationRecord;
use crate::service::ApiResult;
use crate::service::ExplainTask;
use crate::service::Inner;
use crate::service::explain_group;
use crate::service::now_ms;

/// 详细讲解在等待引擎结果期间需要保留的上下文。
struct ExplainJob {
    study_id: String,
    node_id: NodeId,
    mv: Move,
    budget: Budget,
    before: Position,
    ctx: ExplainContext,
}

#[derive(Serialize)]
struct ExplanationEvent<'a> {
    seq: u64,
    study_id: &'a str,
    node_id: NodeId,
    status: ExplainStatus,
    record: &'a ExplanationRecord,
    stats: ExplainStats,
}

#[derive(Serialize)]
struct ProgressEvent<'a> {
    request_id: &'a str,
    study_id: &'a str,
    node_id: NodeId,
    depth: u32,
    nodes: u64,
    time_ms: u64,
    candidates: Vec<CandidateView>,
}

#[derive(Serialize)]
struct AnalysisDoneEvent<'a> {
    request_id: &'a str,
    study_id: &'a str,
    node_id: NodeId,
    budget: Budget,
    position: PositionExplanation,
    error: Option<String>,
    cancelled: bool,
}

impl Inner {
    pub(crate) fn engine_identity(&self) -> Result<EngineIdentity, String> {
        match self.engine.state() {
            EngineState::Ready { identity, .. } => Ok(identity),
            EngineState::Starting => Err("引擎正在启动，请稍候".into()),
            EngineState::Failed { reason } => Err(format!("引擎不可用：{reason}")),
            EngineState::Stopped => Err("引擎已停止".into()),
        }
    }

    pub(crate) fn conditions(
        &self, study: &Study, node: NodeId, budget: Budget, searchmoves: Vec<Move>,
    ) -> ApiResult<AnalysisConditions> {
        let identity = self.engine_identity()?;
        let (base_fen, moves) = study.engine_history(node)?;
        let cfg = self.config_snapshot();
        let (depth, movetime_ms) = match budget {
            Budget::Quick => (cfg.analysis.quick_depth, cfg.analysis.quick_time_ms),
            Budget::Deep => (cfg.analysis.deep_depth, cfg.analysis.deep_time_ms),
        };
        let multipv = if searchmoves.is_empty() { cfg.analysis.multipv } else { 1 };
        Ok(AnalysisConditions {
            base_fen,
            moves,
            searchmoves,
            budget,
            multipv,
            depth,
            movetime_ms,
            rules: cfg.rules(),
            engine: identity.name,
            nnue: identity.nnue,
            score_type: identity.score_type,
        })
    }

    /// 先查缓存，未命中再提交引擎任务；完成的结果写入数据库后回调。
    fn search(
        &self, conditions: AnalysisConditions, priority: Priority, group: String,
        on_progress: Option<Box<dyn FnMut(SearchProgress) + Send>>, on_done: impl FnOnce(SearchOutcome) + Send + 'static,
    ) {
        let key = conditions.key();
        if let Ok(Some(record)) = self.store.load_analysis(&key) {
            on_done(match record.outcome {
                AnalysisOutcome::Completed { result } => SearchOutcome::Completed(result),
                AnalysisOutcome::Terminal { status } => SearchOutcome::Terminal(status),
            });
            return;
        }
        let request = SearchRequest {
            base_fen: conditions.base_fen.clone(),
            moves: conditions.moves.clone(),
            multipv: conditions.multipv,
            depth: Some(conditions.depth),
            movetime_ms: Some(conditions.movetime_ms),
            searchmoves: conditions.searchmoves.clone(),
        };
        let weak = self.weak.clone();
        self.engine.submit(JobSpec {
            priority,
            group,
            request,
            on_progress,
            on_done: Box::new(move |outcome| {
                if let Some(inner) = weak.upgrade() {
                    let stored = match &outcome {
                        SearchOutcome::Completed(r) => Some(AnalysisOutcome::Completed { result: r.clone() }),
                        SearchOutcome::Terminal(s) => Some(AnalysisOutcome::Terminal { status: *s }),
                        _ => None,
                    };
                    if let Some(outcome) = stored {
                        let record = AnalysisRecord { key, conditions, outcome, created_at: now_ms() };
                        if let Err(e) = inner.store.save_analysis(&record) {
                            warn!("保存分析结果失败：{e}");
                        }
                    }
                }
                on_done(outcome);
            }),
        });
    }

    /// 已缓存的局面分析（优先使用深入分析）。
    pub(crate) fn cached_position_evidence(&self, study: &Study, node: NodeId) -> Option<EngineEvidence> {
        for budget in [Budget::Deep, Budget::Quick] {
            let Ok(cond) = self.conditions(study, node, budget, Vec::new()) else { return None };
            if let Ok(Some(record)) = self.store.load_analysis(&cond.key())
                && let AnalysisOutcome::Completed { result } = record.outcome
            {
                return Some(EngineEvidence { candidates: result.lines, actual: None, conditions: cond.describe() });
            }
        }
        None
    }

    pub(crate) fn analyze_position(&self, req: AnalyzeRequest) -> ApiResult<()> {
        let study = self.with_study(&req.study_id, |s| Ok(s.clone()))?;
        let pos = study.position(req.node_id)?;
        let ctx = self.context_at(&study, req.node_id)?;
        let conditions = self.conditions(&study, req.node_id, req.budget, Vec::new())?;
        let describe = conditions.describe();
        let events_sink = self.events.clone();
        let progress_pos = pos.clone();
        let (rid, sid, nid) = (req.request_id.clone(), req.study_id.clone(), req.node_id);
        let progress: Box<dyn FnMut(SearchProgress) + Send> = Box::new(move |p: SearchProgress| {
            events::emit(events_sink.as_ref(), events::ANALYSIS_PROGRESS, &ProgressEvent {
                request_id: &rid,
                study_id: &sid,
                node_id: nid,
                depth: p.depth,
                nodes: p.nodes,
                time_ms: p.time_ms,
                candidates: explain::candidate_list(&progress_pos, &p.lines),
            });
        });
        let events_sink = self.events.clone();
        let budget = req.budget;
        self.search(conditions, Priority::Interactive, format!("analyze:{}", req.request_id), Some(progress), move |outcome| {
            let (engine, error, cancelled) = match outcome {
                SearchOutcome::Completed(r) => {
                    (Some(EngineEvidence { candidates: r.lines, actual: None, conditions: describe }), None, false)
                }
                SearchOutcome::Terminal(_) => (None, None, false),
                SearchOutcome::Cancelled => (None, None, true),
                SearchOutcome::Failed(e) => (None, Some(e), false),
            };
            let position = explain::explain_position(&pos, &ctx, engine.as_ref());
            events::emit(events_sink.as_ref(), events::ANALYSIS_DONE, &AnalysisDoneEvent {
                request_id: &req.request_id,
                study_id: &req.study_id,
                node_id: req.node_id,
                budget,
                position,
                error,
                cancelled,
            });
        });
        Ok(())
    }

    fn priority_for(&self, study_id: &str, node_id: NodeId) -> Priority {
        let focus = self.focus.lock().map(|f| f.clone()).unwrap_or(None);
        match focus {
            Some((s, n)) if s == study_id && n == node_id => Priority::Interactive,
            _ => Priority::Background,
        }
    }

    fn node_exists(&self, study_id: &str, node_id: NodeId) -> bool {
        self.studies.lock().map(|s| s.get(study_id).is_some_and(|st| st.nodes.contains_key(&node_id))).unwrap_or(false)
    }

    fn save_and_emit(&self, record: &mut ExplanationRecord) {
        if !self.node_exists(&record.study_id, record.node_id) {
            return;
        }
        record.updated_at = now_ms();
        if let Err(e) = self.store.save_explanation(record) {
            warn!("保存讲解失败：{e}");
            return;
        }
        let seq = self.explain_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        events::emit(self.events.as_ref(), events::EXPLANATION_UPDATED, &ExplanationEvent {
            seq,
            study_id: &record.study_id,
            node_id: record.node_id,
            status: record.status(),
            record,
            stats: self.explain_stats(&record.study_id),
        });
    }

    fn load_record(&self, study_id: &str, node_id: NodeId) -> ExplanationRecord {
        self.store
            .load_explanation(study_id, node_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| ExplanationRecord::new(study_id, node_id, now_ms()))
    }

    pub(crate) fn process_explain(&self, task: ExplainTask) {
        let Ok(study) = self.with_study(&task.study_id, |s| Ok(s.clone())) else { return };
        let Some(node) = study.nodes.get(&task.node_id) else { return };
        let (Some(parent), Some(mv)) = (node.parent, node.mv) else { return };
        let (before, ctx) = match (study.position(parent), self.context_at(&study, parent)) {
            (Ok(p), Ok(c)) => (p, c),
            _ => return,
        };
        let mut record = self.load_record(&task.study_id, task.node_id);
        // 讲解模板更新后，旧版本的讲解重新生成
        if record.explanation.as_ref().is_some_and(|e| e.generator_version != explain::GENERATOR_VERSION) {
            record.explanation = None;
            record.budget = None;
        }
        if record.explanation.is_none() {
            record.explanation = explain::explain_move(&before, mv, &ctx, None);
            if record.explanation.is_none() {
                record.pending = false;
                record.error = Some("无法生成讲解：着法与局面不一致".into());
                self.save_and_emit(&mut record);
                return;
            }
        }
        if !task.detailed || record.is_detailed_for(task.budget) {
            record.pending = false;
            self.save_and_emit(&mut record);
            return;
        }
        let key = (task.study_id.clone(), task.node_id);
        let group = explain_group(&task.study_id, task.node_id);
        {
            let mut inflight = self.inflight.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(running) = inflight.get(&key)
                && (*running == Budget::Deep || task.budget == Budget::Quick)
            {
                if task.priority == Priority::Interactive {
                    self.engine.reprioritize(&group, Priority::Interactive);
                }
                return;
            }
            inflight.insert(key.clone(), task.budget);
        }
        let conditions = match self.conditions(&study, parent, task.budget, Vec::new()) {
            Ok(c) => c,
            Err(e) => {
                self.inflight.lock().unwrap_or_else(|p| p.into_inner()).remove(&key);
                record.pending = false;
                record.error = Some(e.message);
                self.save_and_emit(&mut record);
                return;
            }
        };
        record.pending = true;
        record.error = None;
        self.save_and_emit(&mut record);
        let job = ExplainJob { study_id: task.study_id, node_id: task.node_id, mv, budget: task.budget, before, ctx };
        let weak = self.weak.clone();
        let cond = conditions.clone();
        self.search(conditions, task.priority, group, None, move |outcome| {
            if let Some(inner) = weak.upgrade() {
                inner.after_multipv(job, cond, outcome);
            }
        });
    }

    fn after_multipv(&self, job: ExplainJob, cond: AnalysisConditions, outcome: SearchOutcome) {
        match outcome {
            SearchOutcome::Completed(result) => {
                if result.lines.iter().any(|l| l.mv == job.mv) {
                    self.finish_explain(job, &cond, result.lines, None);
                    return;
                }
                // 实际走法不在候选中：相同预算下单独分析它
                let single = AnalysisConditions { searchmoves: vec![job.mv], multipv: 1, ..cond.clone() };
                let priority = self.priority_for(&job.study_id, job.node_id);
                let group = explain_group(&job.study_id, job.node_id);
                let weak = self.weak.clone();
                let lines = result.lines;
                self.search(single, priority, group, None, move |outcome| {
                    if let Some(inner) = weak.upgrade() {
                        let actual = match outcome {
                            SearchOutcome::Completed(r) => r.lines.into_iter().next(),
                            SearchOutcome::Cancelled => {
                                inner.finish_without_engine(job, None);
                                return;
                            }
                            _ => None,
                        };
                        inner.finish_explain(job, &cond, lines, actual);
                    }
                });
            }
            SearchOutcome::Cancelled => self.finish_without_engine(job, None),
            SearchOutcome::Terminal(_) => self.finish_without_engine(job, Some("落子前的局面已经终局，无法分析".into())),
            SearchOutcome::Failed(e) => self.finish_without_engine(job, Some(e)),
        }
    }

    fn finish_explain(&self, job: ExplainJob, cond: &AnalysisConditions, candidates: Vec<EngineLine>, actual: Option<EngineLine>) {
        let engine = EngineEvidence { candidates, actual, conditions: cond.describe() };
        let mut record = self.load_record(&job.study_id, job.node_id);
        if let Some(expl) = explain::explain_move(&job.before, job.mv, &job.ctx, Some(&engine)) {
            record.explanation = Some(expl);
            record.budget = Some(job.budget);
        }
        record.pending = false;
        record.error = None;
        self.inflight.lock().unwrap_or_else(|p| p.into_inner()).remove(&(job.study_id.clone(), job.node_id));
        self.save_and_emit(&mut record);
    }

    /// 引擎任务没有完成：保留基本讲解，记录原因（取消时不算失败）。
    fn finish_without_engine(&self, job: ExplainJob, error: Option<String>) {
        let mut record = self.load_record(&job.study_id, job.node_id);
        record.pending = false;
        record.error = error;
        self.inflight.lock().unwrap_or_else(|p| p.into_inner()).remove(&(job.study_id.clone(), job.node_id));
        self.save_and_emit(&mut record);
    }
}
