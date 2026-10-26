//! 分析与讲解流水线。
//!
//! 讲解某一着分三步：先用规则事实生成基本讲解；再对落子前局面做多候选搜索；
//! 实际走法不在候选中时，用 searchmoves 在相同预算下单独分析。搜索结果按完整条件缓存，
//! 只有完整一致的结果才会被复用。
//!
//! 讲解任务的每一步都在讲解线程上执行（引擎回调只把结果交回该线程），
//! 因此“确认仍是最新请求、写入记录”不会与新请求的登记交错。

use serde::Serialize;
use std::sync::atomic::Ordering;

use tracing::debug;
use tracing::warn;
use xq_core::Move;
use xq_core::Position;
use xq_core::explain;
use xq_core::explain::CandidateView;
use xq_core::explain::EngineEvidence;
use xq_core::explain::ExplainContext;
use xq_core::explain::PositionExplanation;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::api::AnalyzeRequest;
use crate::api::ExplainStats;
use crate::engine::EngineExpectation;
use crate::engine::EngineIdentity;
use crate::engine::EngineState;
use crate::engine::JobSpec;
use crate::engine::Priority;
use crate::engine::SearchOutcome;
use crate::engine::SearchProgress;
use crate::engine::SearchRequest;
use crate::engine::SearchResult;
use crate::events;
use crate::records::ANALYSIS_FORMAT;
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
    priority: Priority,
    /// 该节点讲解请求的代次，用于识别被更新请求取代的旧结果。
    generation: u64,
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
        let (base_fen, moves) = study.engine_history(node)?;
        self.conditions_from(base_fen, moves, budget, searchmoves)
    }

    /// 从给定的起始局面与着法历史得到分析条件（复习卡片的局面可能已经不在任何会话中）。
    pub(crate) fn conditions_from(
        &self, base_fen: String, moves: Vec<Move>, budget: Budget, searchmoves: Vec<Move>,
    ) -> ApiResult<AnalysisConditions> {
        let identity = self.engine_identity()?;
        let cfg = self.config_snapshot();
        let (depth, movetime_ms) = match budget {
            Budget::Quick => (cfg.analysis.quick_depth, cfg.analysis.quick_time_ms),
            Budget::Deep => (cfg.analysis.deep_depth, cfg.analysis.deep_time_ms),
        };
        // 限定着法时每一步各给出一条变化（实际着法单独分析时为 1，对比两步时为 2）
        let multipv = if searchmoves.is_empty() { cfg.analysis.multipv } else { searchmoves.len() as u32 };
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
    ///
    /// 缓存只复用完整一致的结果（以及终局）。不完整的结果仍会保存用于展示，但再次请求时重新搜索，
    /// 用户重试才能真正得到新的分析。
    pub(crate) fn search(
        &self, conditions: AnalysisConditions, priority: Priority, group: String,
        on_progress: Option<Box<dyn FnMut(SearchProgress) + Send>>, on_done: impl FnOnce(SearchOutcome) + Send + 'static,
    ) {
        let key = conditions.key();
        if let Ok(Some(record)) = self.store.load_analysis(&key)
            && record.reusable()
        {
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
            expect: Some(EngineExpectation {
                engine: conditions.engine.clone(),
                nnue: conditions.nnue.clone(),
                score_type: conditions.score_type.clone(),
                rules: conditions.rules.clone(),
            }),
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
                        // 同一条件的另一次搜索可能已经得到完整结果，不完整的结果不覆盖它
                        SearchOutcome::Completed(r)
                            if !r.complete
                                && matches!(inner.store.load_analysis(&key), Ok(Some(existing)) if existing.reusable()) =>
                        {
                            None
                        }
                        SearchOutcome::Completed(r) => Some(AnalysisOutcome::Completed { result: r.clone() }),
                        SearchOutcome::Terminal(s) => Some(AnalysisOutcome::Terminal { status: *s }),
                        _ => None,
                    };
                    if let Some(outcome) = stored {
                        let record =
                            AnalysisRecord { key, conditions, outcome, format: ANALYSIS_FORMAT, created_at: now_ms() };
                        if let Err(e) = inner.store.save_analysis(&record) {
                            warn!("保存分析结果失败：{e}");
                        }
                    }
                }
                on_done(outcome);
            }),
        });
    }

    /// 已缓存的局面分析：完整的结果优先，其次深入分析；不完整的结果也展示，但讲解会注明尚未完成。
    pub(crate) fn cached_position_evidence(&self, study: &Study, node: NodeId) -> Option<EngineEvidence> {
        let mut partial = None;
        for budget in [Budget::Deep, Budget::Quick] {
            let Ok(cond) = self.conditions(study, node, budget, Vec::new()) else { return None };
            if let Ok(Some(record)) = self.store.load_analysis(&cond.key())
                && record.is_current_format()
                && let AnalysisOutcome::Completed { result } = record.outcome
            {
                let engine = evidence(result, None, &cond);
                if engine.complete {
                    return Some(engine);
                }
                partial.get_or_insert(engine);
            }
        }
        partial
    }

    pub(crate) fn analyze_position(&self, req: AnalyzeRequest) -> ApiResult<()> {
        let study = self.with_study(&req.study_id, |s| Ok(s.clone()))?;
        let pos = study.position(req.node_id)?;
        let ctx = self.context_at(&study, req.node_id)?;
        let conditions = self.conditions(&study, req.node_id, req.budget, Vec::new())?;
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
        let weak = self.weak.clone();
        let cond = conditions.clone();
        self.search(conditions, Priority::Interactive, format!("analyze:{}", req.request_id), Some(progress), move |outcome| {
            let (engine, error, cancelled) = match outcome {
                SearchOutcome::Completed(r) => (Some(evidence(r, None, &cond)), None, false),
                SearchOutcome::Terminal(_) => (None, None, false),
                SearchOutcome::Cancelled => (None, None, true),
                SearchOutcome::Failed(e) => (None, Some(e), false),
                SearchOutcome::Stale(reason) => {
                    // 设置在排队期间改变：按新设置重新分析，界面继续等待同一个请求
                    let retry = weak.upgrade().map(|inner| inner.analyze_position(req.clone()));
                    match retry {
                        Some(Ok(())) => return,
                        Some(Err(e)) => (None, Some(format!("{reason}，重新分析失败：{}", e.message)), false),
                        None => (None, Some(reason), false),
                    }
                }
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

    pub(crate) fn node_exists(&self, study_id: &str, node_id: NodeId) -> bool {
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
        self.coach_on_explanation(record);
    }

    fn load_record(&self, study_id: &str, node_id: NodeId) -> ExplanationRecord {
        self.store
            .load_explanation(study_id, node_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| ExplanationRecord::new(study_id, node_id, now_ms()))
    }

    /// 已有的讲解是否仍对应当前设置、不需要重新分析：模板版本、分析预算与分析条件都要一致。
    ///
    /// 部分完成的讲解在用户明确重试前同样保留，避免每次查看都重新搜索；重试时不复用不完整的分析。
    /// 引擎暂不可用时无法核对，保留已有讲解。
    fn record_is_current(
        &self, record: &ExplanationRecord, study: &Study, parent: NodeId, requested: Budget, retry: bool,
    ) -> bool {
        match record.status() {
            ExplainStatus::Detailed => {}
            ExplainStatus::Partial if !retry => {}
            _ => return false,
        }
        let Some(budget) = record.budget else { return false };
        if requested == Budget::Deep && budget != Budget::Deep {
            return false;
        }
        if record.explanation.as_ref().is_some_and(|e| e.generator_version != explain::GENERATOR_VERSION) {
            return false;
        }
        match self.conditions(study, parent, budget, Vec::new()) {
            Ok(c) => record.conditions_key.as_deref() == Some(c.key().as_str()),
            Err(_) => true,
        }
    }

    /// 该任务是否仍是节点最新的讲解请求。
    fn is_latest(&self, job: &ExplainJob) -> bool {
        let inflight = self.inflight.lock().unwrap_or_else(|p| p.into_inner());
        matches!(inflight.get(&(job.study_id.clone(), job.node_id)), Some((_, generation)) if *generation == job.generation)
    }

    fn end_job(&self, job: &ExplainJob) {
        self.inflight.lock().unwrap_or_else(|p| p.into_inner()).remove(&(job.study_id.clone(), job.node_id));
    }

    /// 提交讲解任务的结果：确认它仍是节点最新的请求，写入记录，再结束登记。
    /// 被更新的请求取代时丢弃结果。只在讲解线程上调用，新请求的登记不会插在确认与写入之间。
    fn commit(&self, job: &ExplainJob, update: impl FnOnce(&mut ExplanationRecord)) {
        if !self.is_latest(job) {
            debug!("节点 {} 的讲解结果已被更新的请求取代，丢弃", job.node_id);
            return;
        }
        let hook = self.commit_hook.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(hook) = hook {
            hook(&job.study_id, job.node_id, job.budget);
        }
        let mut record = self.load_record(&job.study_id, job.node_id);
        update(&mut record);
        self.save_and_emit(&mut record);
        self.end_job(job);
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
        // 只有记录确实变化时才写入并推送，避免每次查看节点都产生写入
        let mut changed = false;
        // 讲解模板更新后，旧版本的讲解重新生成
        if record.explanation.as_ref().is_some_and(|e| e.generator_version != explain::GENERATOR_VERSION) {
            record.explanation = None;
            record.budget = None;
            record.conditions_key = None;
        }
        if record.explanation.is_none() {
            changed = true;
            record.explanation = explain::explain_move(&before, mv, &ctx, None);
            if record.explanation.is_none() {
                record.pending = false;
                record.error = Some("无法生成讲解：着法与局面不一致".into());
                self.save_and_emit(&mut record);
                return;
            }
        }
        let key = (task.study_id.clone(), task.node_id);
        let group = explain_group(&task.study_id, task.node_id);
        let running = self.inflight.lock().unwrap_or_else(|p| p.into_inner()).get(&key).copied();
        if !task.detailed || self.record_is_current(&record, &study, parent, task.budget, task.retry) {
            if running.is_none() && record.pending {
                record.pending = false;
                changed = true;
            }
            if changed {
                self.save_and_emit(&mut record);
            }
            return;
        }
        if let Some((budget, _)) = running
            && (budget == Budget::Deep || task.budget == Budget::Quick)
        {
            // 已有覆盖本次请求的任务在进行
            if task.priority == Priority::Interactive {
                self.engine.reprioritize(&group, Priority::Interactive);
            }
            if changed {
                self.save_and_emit(&mut record);
            }
            return;
        }
        let conditions = match self.conditions(&study, parent, task.budget, Vec::new()) {
            Ok(c) => c,
            Err(e) => {
                if running.is_none() {
                    record.pending = false;
                }
                record.error = Some(e.message);
                self.save_and_emit(&mut record);
                return;
            }
        };
        let generation = self.explain_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.inflight.lock().unwrap_or_else(|p| p.into_inner()).insert(key, (task.budget, generation));
        record.pending = true;
        record.error = None;
        self.save_and_emit(&mut record);
        let job = ExplainJob {
            study_id: task.study_id,
            node_id: task.node_id,
            mv,
            budget: task.budget,
            priority: task.priority,
            generation,
            before,
            ctx,
        };
        let weak = self.weak.clone();
        let cond = conditions.clone();
        self.search(conditions, task.priority, group, None, move |outcome| {
            if let Some(inner) = weak.upgrade() {
                inner.on_explain_thread(move |inner| inner.after_multipv(job, cond, outcome));
            }
        });
    }

    fn after_multipv(&self, job: ExplainJob, cond: AnalysisConditions, outcome: SearchOutcome) {
        match outcome {
            SearchOutcome::Completed(result) => {
                if result.lines.iter().any(|l| l.mv == job.mv) {
                    self.finish_explain(job, &cond, result, None, None);
                    return;
                }
                // 实际走法不在候选中：相同预算下单独分析它
                let single = AnalysisConditions { searchmoves: vec![job.mv], multipv: 1, ..cond.clone() };
                let priority = self.priority_for(&job.study_id, job.node_id);
                let group = explain_group(&job.study_id, job.node_id);
                let weak = self.weak.clone();
                self.search(single, priority, group, None, move |outcome| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_explain_thread(move |inner| inner.after_actual(job, cond, result, outcome));
                    }
                });
            }
            SearchOutcome::Cancelled => self.finish_without_engine(job, None),
            SearchOutcome::Stale(_) => self.restart_explain(job),
            SearchOutcome::Terminal(_) => self.finish_without_engine(job, Some("落子前的局面已经终局，无法分析".into())),
            SearchOutcome::Failed(e) => self.finish_without_engine(job, Some(format!("候选分析失败：{e}"))),
        }
    }

    /// 实际走法单独分析结束。结果的首着必须就是实际走法，完整性与说明随结果一起保留。
    fn after_actual(&self, job: ExplainJob, cond: AnalysisConditions, result: SearchResult, outcome: SearchOutcome) {
        match outcome {
            SearchOutcome::Completed(single) => match single.lines.first() {
                Some(line) if line.mv == job.mv => self.finish_explain(job, &cond, result, Some(single), None),
                Some(_) => {
                    let error = "实际着法单独分析返回了其他着法".to_string();
                    self.finish_explain(job, &cond, result, None, Some(error))
                }
                None => self.finish_explain(job, &cond, result, None, Some("实际着法单独分析没有返回结果".into())),
            },
            SearchOutcome::Cancelled => self.finish_without_engine(job, None),
            SearchOutcome::Stale(_) => self.restart_explain(job),
            SearchOutcome::Terminal(_) => {
                self.finish_explain(job, &cond, result, None, Some("实际着法单独分析没有返回结果".into()))
            }
            SearchOutcome::Failed(e) => {
                self.finish_explain(job, &cond, result, None, Some(format!("实际着法单独分析失败：{e}")))
            }
        }
    }

    /// 引擎设置在排队期间改变：按新设置重新排队，讲解仍保持进行中。
    fn restart_explain(&self, job: ExplainJob) {
        if self.is_latest(&job) {
            self.end_job(&job);
            self.enqueue(&job.study_id, job.node_id, job.budget, job.priority, true);
        }
    }

    fn finish_explain(
        &self, job: ExplainJob, cond: &AnalysisConditions, result: SearchResult, single: Option<SearchResult>,
        stage_error: Option<String>,
    ) {
        let engine = evidence(result, single, cond);
        self.commit(&job, |record| {
            if let Some(expl) = explain::explain_move(&job.before, job.mv, &job.ctx, Some(&engine)) {
                record.explanation = Some(expl);
                record.budget = Some(job.budget);
                record.conditions_key = Some(cond.key());
            }
            record.pending = false;
            record.error = stage_error;
        });
    }

    /// 引擎任务没有完成：保留已有讲解，记录原因（取消时不算失败）。
    fn finish_without_engine(&self, job: ExplainJob, error: Option<String>) {
        self.commit(&job, |record| {
            record.pending = false;
            record.error = error;
        });
    }
}

/// 把搜索结果整理成讲解用的引擎证据，保留结果是否完整一致的信息：
/// 实际走法单独分析的完整性与说明同样计入，任何一次搜索不完整，证据都只算部分完成。
fn evidence(result: SearchResult, single: Option<SearchResult>, cond: &AnalysisConditions) -> EngineEvidence {
    let mut complete = result.complete;
    let mut notes = result.notes;
    let actual = single.and_then(|s| {
        complete &= s.complete;
        notes.extend(s.notes.into_iter().map(|n| format!("实际着法单独分析：{n}")));
        s.lines.into_iter().next()
    });
    EngineEvidence { candidates: result.lines, actual, conditions: cond.describe(), complete, notes }
}
