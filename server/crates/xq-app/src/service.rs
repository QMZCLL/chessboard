//! 应用服务：学习会话、走子、分析与讲解任务、屏幕连接的统一入口。

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::RwLock;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::thread;

use serde::Serialize;
use tracing::error;
use tracing::warn;
use xq_core::Color;
use xq_core::Position;
use xq_core::explain::ExplainContext;
use xq_core::fen;
use xq_core::record;
use xq_core::rules;
use xq_core::study::MoveSource;
use xq_core::study::NewStudy;
use xq_core::study::NodeId;
use xq_core::study::Orientation;
use xq_core::study::ROOT;
use xq_core::study::Study;
use xq_core::study::StudyError;
use xq_core::study::StudySource;
use xq_core::validate;

use crate::api::*;
use crate::capture::CaptureBackend;
use crate::capture::CaptureSession;
use crate::capture::CaptureState;
use crate::config;
use crate::config::AppConfig;
use crate::engine::EngineService;
use crate::engine::EngineSpec;
use crate::engine::EngineState;
use crate::engine::Priority;
use crate::events;
use crate::events::EventSink;
use crate::llm::service::CoachRuntime;
use crate::records::Budget;
use crate::records::ExplainStatus;
use crate::records::ExplanationRecord;
use crate::review::ReviewAttemptRequest;
use crate::review::ReviewDetail;
use crate::review::ReviewItem;
use crate::review::ReviewRef;
use crate::review::ReviewUpdateRequest;
use crate::storage::Store;
use crate::storage::StoreError;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Serialize)]
pub struct ApiError {
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.message) }
}

impl std::error::Error for ApiError {}

impl From<String> for ApiError {
    fn from(message: String) -> Self { Self { message } }
}

impl From<&str> for ApiError {
    fn from(message: &str) -> Self { Self { message: message.to_string() } }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self { Self { message: e.to_string() } }
}

impl From<StudyError> for ApiError {
    fn from(e: StudyError) -> Self { Self { message: e.to_string() } }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// 应用使用的目录。`resource_dir` 下应有 `libs/pikafish` 与 `libs/models`。
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
    pub resource_dir: PathBuf,
    /// 0.1.x 版本的配置文件位置。
    pub legacy_config: Option<PathBuf>,
}

impl AppPaths {
    pub fn engine_dir(&self) -> PathBuf { self.resource_dir.join("libs").join("pikafish") }

    pub fn model_dir(&self) -> PathBuf { self.resource_dir.join("libs").join("models") }

    pub fn bundled_engine(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "pikafish-windows.exe"
        } else if cfg!(target_os = "macos") {
            "pikafish-macos"
        } else {
            "pikafish-linux"
        };
        self.engine_dir().join(name)
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub(crate) fn explain_group(study: &str, node: NodeId) -> String { format!("explain:{study}:{node}") }

/// 新会话的起点：局面、导入时给出的计数字段，以及此前历史是否完整。
pub(crate) struct StudyStart {
    pub pos: Position,
    pub counters: Option<(u32, u32)>,
    pub history_complete: bool,
}

pub(crate) struct ExplainTask {
    pub study_id: String,
    pub node_id: NodeId,
    pub budget: Budget,
    pub priority: Priority,
    /// false 时只生成基本讲解。
    pub detailed: bool,
    /// 用户明确要求重试：部分完成的讲解重新分析。
    pub retry: bool,
}

/// 讲解线程处理的消息。
///
/// 引擎任务结束后的处理也交回讲解线程执行：确认代次、写入讲解与登记新请求都在同一线程上依次进行，
/// 已被取代的旧结果不会在新结果写入之后才落盘。
pub(crate) enum ExplainMsg {
    Task(ExplainTask),
    Continue(Box<dyn FnOnce(&Inner) + Send>),
}

/// 测试用的钩子：讲解结果确认仍是最新请求、尚未写入时调用，参数为会话、节点与分析预算。
pub type CommitHook = Arc<dyn Fn(&str, NodeId, Budget) + Send + Sync>;

pub(crate) struct Inner {
    pub(crate) paths: AppPaths,
    pub(crate) config: RwLock<AppConfig>,
    pub(crate) config_path: PathBuf,
    pub(crate) notices: Mutex<VecDeque<String>>,
    pub(crate) store: Store,
    pub(crate) studies: Mutex<HashMap<String, Study>>,
    pub(crate) engine: EngineService,
    pub(crate) events: Arc<dyn EventSink>,
    /// 正在进行的详细讲解：(会话, 节点) → (预算, 请求代次)。
    pub(crate) inflight: Mutex<HashMap<(String, NodeId), (Budget, u64)>>,
    pub(crate) explain_generation: AtomicU64,
    pub(crate) focus: Mutex<Option<(String, NodeId)>>,
    /// 每次讲解记录变化递增；视图与事件都带上它，界面据此识别过期的视图。
    pub(crate) explain_seq: AtomicU64,
    pub(crate) explain_tx: Mutex<Sender<ExplainMsg>>,
    pub(crate) commit_hook: Mutex<Option<CommitHook>>,
    pub(crate) coach: CoachRuntime,
    pub(crate) capture_backend: Arc<dyn CaptureBackend>,
    pub(crate) capture: Mutex<Option<CaptureSession>>,
    pub(crate) capture_state: Mutex<CaptureState>,
    pub(crate) weak: Weak<Inner>,
}

/// 服务入口；可在多个线程间共享。
#[derive(Clone)]
pub struct AppService {
    pub(crate) inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> { m.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) }

pub(crate) fn engine_spec(paths: &AppPaths, cfg: &AppConfig) -> EngineSpec {
    EngineSpec {
        binary: cfg.engine.binary.clone().unwrap_or_else(|| paths.bundled_engine()),
        args: Vec::new(),
        nnue: Some(cfg.engine.nnue.clone().unwrap_or_else(|| paths.engine_dir().join("pikafish.nnue"))),
        threads: cfg.engine.threads,
        hash_mb: cfg.engine.hash_mb,
        rules: cfg.rules(),
    }
}

impl AppService {
    pub fn start(paths: AppPaths, events: Arc<dyn EventSink>, capture_backend: Arc<dyn CaptureBackend>) -> ApiResult<Self> {
        let loaded = config::load(&paths.config_dir, paths.legacy_config.as_deref());
        let store = Store::open(&paths.data_dir.join("xqlink.redb"))?;
        // 上次退出时尚未完成的 AI 讲解请求已经不存在，标为中断，避免界面一直显示排队或生成中
        match store.reset_interrupted_coach(now_ms()) {
            Ok(0) => {}
            Ok(n) => tracing::info!("{n} 条 AI 讲解请求在上次退出时中断，已标为可重新生成"),
            Err(e) => warn!("整理中断的 AI 讲解请求失败：{e}"),
        }
        let engine_events = events.clone();
        let engine = EngineService::start(
            engine_spec(&paths, &loaded.config),
            Box::new(move |state: &EngineState| events::emit(engine_events.as_ref(), events::ENGINE_STATUS, state)),
        )?;
        let (tx, rx) = mpsc::channel::<ExplainMsg>();
        let (coach_tx, coach_rx) = mpsc::channel();
        let inner = Arc::new_cyclic(|weak| Inner {
            paths,
            config: RwLock::new(loaded.config),
            config_path: loaded.path,
            notices: Mutex::new(loaded.notes.into_iter().collect()),
            store,
            studies: Mutex::new(HashMap::new()),
            engine,
            events,
            inflight: Mutex::new(HashMap::new()),
            explain_generation: AtomicU64::new(0),
            focus: Mutex::new(None),
            explain_seq: AtomicU64::new(0),
            explain_tx: Mutex::new(tx),
            commit_hook: Mutex::new(None),
            coach: CoachRuntime::new(coach_tx),
            capture_backend,
            capture: Mutex::new(None),
            capture_state: Mutex::new(CaptureState::Idle),
            weak: weak.clone(),
        });
        let weak = Arc::downgrade(&inner);
        thread::Builder::new()
            .name("explain".into())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    let Some(inner) = weak.upgrade() else { break };
                    let what = match &msg {
                        ExplainMsg::Task(task) => format!("会话 {}，节点 {}", task.study_id, task.node_id),
                        ExplainMsg::Continue(_) => "处理引擎结果".to_string(),
                    };
                    let run = std::panic::AssertUnwindSafe(|| match msg {
                        ExplainMsg::Task(task) => inner.process_explain(task),
                        ExplainMsg::Continue(step) => step(&inner),
                    });
                    // 单个讲解任务出错时记录下来，讲解线程继续处理其他任务
                    if std::panic::catch_unwind(run).is_err() {
                        inner.notice(format!("生成讲解时出错（{what}），已跳过"));
                    }
                }
            })
            .map_err(|e| format!("无法启动讲解线程：{e}"))?;
        crate::llm::service::spawn_workers(Arc::downgrade(&inner), coach_rx)?;
        Ok(Self { inner })
    }

    pub fn shutdown(&self) {
        if let Some(session) = lock(&self.inner.capture).take() {
            session.stop();
        }
        self.inner.engine.shutdown();
    }

    pub fn status(&self) -> AppStatus {
        let inner = &self.inner;
        AppStatus {
            version: APP_VERSION.to_string(),
            engine: inner.engine.state(),
            recognition: inner.capture_backend.status(),
            capture: lock(&inner.capture_state).clone(),
            config: inner.config_snapshot(),
            coach: inner.coach_availability(),
            notices: lock(&inner.notices).iter().cloned().collect(),
            log_dir: inner.paths.log_dir.display().to_string(),
            data_dir: inner.paths.data_dir.display().to_string(),
        }
    }

    pub fn log_dir(&self) -> PathBuf { self.inner.paths.log_dir.clone() }

    pub fn update_config(&self, mut cfg: AppConfig) -> ApiResult<AppStatus> {
        let notes = cfg.normalize();
        config::save(&self.inner.config_path, &cfg).map_err(|e| format!("保存设置失败：{e}"))?;
        self.inner.engine.reconfigure(engine_spec(&self.inner.paths, &cfg));
        let old_coach = self.inner.config_snapshot().coach;
        let same_service = old_coach.same_service(&cfg.coach);
        let coach_changed = old_coach != cfg.coach;
        *self.inner.config.write().unwrap_or_else(|p| p.into_inner()) = cfg;
        // 先写入新设置，再递增版本
        if !same_service {
            self.inner.coach_config_changed();
        } else if coach_changed {
            // 学习水平、详细程度等改变不影响请求，但会改变已有结果是否过期
            self.inner.coach_view_changed();
        }
        for n in notes {
            self.inner.notice(n);
        }
        Ok(self.status())
    }

    pub fn restart_engine(&self) { self.inner.engine.restart(); }

    /// 测试用：设置讲解写入前的钩子，用来构造并发交错。
    #[doc(hidden)]
    pub fn set_commit_hook(&self, hook: Option<CommitHook>) { *lock(&self.inner.commit_hook) = hook; }

    pub fn list_studies(&self) -> ApiResult<Vec<StudySummary>> {
        let mut list: Vec<StudySummary> = self
            .inner
            .store
            .list_studies()?
            .into_iter()
            .map(|s| StudySummary {
                moves: s.move_nodes().count(),
                id: s.id,
                name: s.name,
                source: s.source,
                created_at: s.created_at,
                updated_at: s.updated_at,
                root_fen: s.root_fen,
            })
            .collect();
        list.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(list)
    }

    pub fn validate_position(&self, req: ValidateRequest) -> ValidateResponse {
        let parsed = match fen::parse_fen(&req.fen) {
            Ok(p) => p,
            Err(e) => {
                return ValidateResponse { format_error: Some(e.to_string()), report: None, side: None, needs_side: false };
            }
        };
        let side = req.side.or(parsed.side);
        match side {
            Some(side) => ValidateResponse {
                format_error: None,
                report: Some(validate::validate_position(&Position::new(parsed.board, side))),
                side: Some(side),
                needs_side: false,
            },
            None => {
                let issues = validate::validate_board(&parsed.board);
                let ok = issues.iter().all(|i| i.severity != validate::Severity::Error);
                ValidateResponse {
                    format_error: None,
                    report: Some(validate::ValidationReport { ok, issues, status: None, in_check: false }),
                    side: None,
                    needs_side: true,
                }
            }
        }
    }

    pub fn create_study(&self, req: CreateStudyRequest) -> ApiResult<ImportResponse> {
        let mut notes = Vec::new();
        let mut stopped = None;
        let (start, source, moves, default_name) = match req.kind {
            CreateKind::Standard => (
                StudyStart { pos: Position::startpos(), counters: None, history_complete: true },
                StudySource::Standard,
                Vec::new(),
                "标准开局".to_string(),
            ),
            CreateKind::Fen { fen: text, side } => {
                let parsed = fen::parse_fen(&text).map_err(|e| e.to_string())?;
                let side = side.or(parsed.side).ok_or("FEN 没有给出行棋方，请选择红走或黑走")?;
                let pos = Position::new(parsed.board, side);
                let name = if pos.is_startpos() { "标准开局".to_string() } else { format!("残局（{}走）", side_short(side)) };
                (StudyStart { pos, counters: parsed.counters, history_complete: false }, StudySource::Fen, Vec::new(), name)
            }
            CreateKind::Setup { board, side } => {
                let parsed = fen::parse_fen(&board).map_err(|e| e.to_string())?;
                let pos = Position::new(parsed.board, side);
                (
                    StudyStart { pos, counters: None, history_complete: false },
                    StudySource::Setup,
                    Vec::new(),
                    format!("摆棋局面（{}走）", side_short(side)),
                )
            }
            CreateKind::Record { text } => {
                let parsed = record::parse_record(&text).map_err(|e| e.to_string())?;
                if let Some(stop) = &parsed.stopped {
                    notes.push(format!("第 {} 步“{}”无法导入：{}，已导入此前的着法", stop.ply, stop.token, stop.reason));
                }
                stopped = parsed.stopped.clone();
                let title = ["Event", "Title", "Name"]
                    .iter()
                    .find_map(|k| parsed.tags.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()))
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "导入的棋谱".to_string());
                let start =
                    StudyStart { pos: parsed.start, counters: parsed.counters, history_complete: !parsed.custom_start };
                (start, StudySource::Record, parsed.moves, title)
            }
        };
        let name = req.name.filter(|n| !n.trim().is_empty()).unwrap_or(default_name);
        let study = self.inner.new_study(name, source, start, req.orientation, req.user_side, &moves)?;
        Ok(ImportResponse { study, notes, stopped })
    }

    pub fn load_study(&self, id: &str) -> ApiResult<StudyView> {
        let view = self.inner.with_study(id, |s| Ok(s.clone()))?;
        // 补上缺少基本讲解的走子（例如上次退出时尚未完成）
        let existing: BTreeMap<NodeId, ExplanationRecord> =
            self.inner.store.explanations_for(id)?.into_iter().map(|r| (r.node_id, r)).collect();
        let auto = self.inner.config_snapshot().analysis.auto_explain;
        let outdated = |r: &ExplanationRecord| {
            r.explanation.as_ref().is_some_and(|e| e.generator_version != xq_core::explain::GENERATOR_VERSION)
        };
        for node in view.move_nodes() {
            match existing.get(&node.id) {
                None => self.inner.enqueue(id, node.id, Budget::Quick, Priority::Background, auto),
                Some(r) if r.pending || outdated(r) => {
                    self.inner.enqueue(id, node.id, r.budget.unwrap_or(Budget::Quick), Priority::Background, true)
                }
                _ => {}
            }
        }
        self.inner.study_view(&view)
    }

    /// 当前会话视图（只读，不排队讲解任务），用于事件后的刷新。
    pub fn study_view(&self, id: &str) -> ApiResult<StudyView> {
        let study = self.inner.with_study(id, |s| Ok(s.clone()))?;
        self.inner.study_view(&study)
    }

    pub fn update_study(&self, req: StudyPrefsRequest) -> ApiResult<StudyView> {
        let study = self.inner.with_study(&req.study_id, |s| {
            if let Some(name) = req.name.filter(|n| !n.trim().is_empty()) {
                s.name = name;
            }
            if let Some(o) = req.orientation {
                s.orientation = o;
            }
            if let Some(side) = req.user_side {
                s.user_side = side;
            }
            s.updated_at = now_ms();
            s.revision += 1;
            Ok(s.clone())
        })?;
        self.inner.study_view(&study)
    }

    pub fn delete_study(&self, id: &str) -> ApiResult<()> {
        if let Some((sid, _, _)) = lock(&self.inner.capture).as_ref().and_then(|c| c.tracking_target())
            && sid == id
        {
            return Err("该学习会话正在记录屏幕对局，请先停止屏幕连接".into());
        }
        let nodes: Vec<NodeId> = self.inner.with_study(id, |s| Ok(s.nodes.keys().copied().collect()))?;
        for n in nodes {
            self.inner.engine.cancel_group(&explain_group(id, n));
        }
        lock(&self.inner.studies).remove(id);
        self.inner.coach_forget(id, None);
        self.inner.store.delete_study(id)?;
        Ok(())
    }

    pub fn node_detail(&self, r: NodeRef) -> ApiResult<NodeDetail> { self.inner.node_detail(&r.study_id, r.node_id) }

    pub fn apply_move(&self, req: MoveRequest) -> ApiResult<MoveResponse> {
        let source = if req.variation { MoveSource::Variation } else { MoveSource::User };
        let result = self.inner.with_study(&req.study_id, |s| match s.apply_move(req.parent, req.mv, source, now_ms()) {
            Ok(outcome) => Ok(Ok((outcome, s.clone()))),
            Err(StudyError::Illegal(reason)) => Ok(Err(reason)),
            Err(e) => Err(e.into()),
        })?;
        match result {
            Ok((outcome, study)) => {
                if outcome.created {
                    self.inner.on_new_nodes(&study, &[outcome.node], "move", Priority::Interactive);
                }
                Ok(MoveResponse {
                    node_id: Some(outcome.node),
                    created: outcome.created,
                    illegal: None,
                    study: Some(self.inner.study_view(&study)?),
                })
            }
            Err(reason) => Ok(MoveResponse { node_id: None, created: false, illegal: Some(reason), study: None }),
        }
    }

    /// 把一串着法（如候选变化）加入为分支。
    pub fn add_line(&self, req: LineRequest) -> ApiResult<MoveResponse> {
        let (created, last, study) = self.inner.with_study(&req.study_id, |s| {
            let mut cur = req.parent;
            let mut created = Vec::new();
            for mv in &req.moves {
                let outcome = s.apply_move(cur, *mv, MoveSource::Variation, now_ms())?;
                if outcome.created {
                    created.push(outcome.node);
                }
                cur = outcome.node;
            }
            Ok((created, cur, s.clone()))
        })?;
        self.inner.on_new_nodes(&study, &created, "line", Priority::Background);
        Ok(MoveResponse { node_id: Some(last), created: !created.is_empty(), illegal: None, study: Some(self.inner.study_view(&study)?) })
    }

    pub fn delete_node(&self, r: NodeRef) -> ApiResult<StudyView> {
        let (removed, study) = self.inner.with_study(&r.study_id, |s| {
            let removed = s.delete_subtree(r.node_id, now_ms())?;
            Ok((removed, s.clone()))
        })?;
        for n in &removed {
            self.inner.engine.cancel_group(&explain_group(&r.study_id, *n));
            lock(&self.inner.inflight).remove(&(r.study_id.clone(), *n));
        }
        self.inner.coach_forget(&r.study_id, Some(&removed));
        self.inner.store.delete_explanations(&r.study_id, &removed)?;
        self.inner.emit_study_updated(&study, "delete", &[]);
        self.inner.study_view(&study)
    }

    pub fn promote_node(&self, r: NodeRef) -> ApiResult<StudyView> {
        let study = self.inner.with_study(&r.study_id, |s| {
            s.promote(r.node_id, now_ms())?;
            Ok(s.clone())
        })?;
        self.inner.study_view(&study)
    }

    pub fn record_practice(&self, req: PracticeRequest) -> ApiResult<StudyView> {
        // 记下这次看过的提示原文：提示与界面显示的一样，由落子前局面的事实与已缓存的候选生成
        let hints = match req.hints_used {
            0 => None,
            level => {
                let study = self.inner.with_study(&req.study_id, |s| Ok(s.clone()))?;
                let parent = study.node(req.node_id)?.parent;
                parent.and_then(|p| {
                    let pos = study.position(p).ok()?;
                    let engine = self.inner.cached_position_evidence(&study, p);
                    xq_core::explain::hints::think_hints(&pos, engine.as_ref())?.used(level.min(3))
                })
            }
        };
        let study = self.inner.with_study(&req.study_id, |s| {
            s.record_practice(req.node_id, req.hints_used, req.revealed_directly, req.first_choice, hints, now_ms())?;
            Ok(s.clone())
        })?;
        self.inner.study_view(&study)
    }

    pub fn set_note(&self, req: NoteRequest) -> ApiResult<StudyView> {
        let study = self.inner.with_study(&req.study_id, |s| {
            s.set_note(req.node_id, req.note, req.starred, now_ms())?;
            Ok(s.clone())
        })?;
        self.inner.study_view(&study)
    }

    /// 用户正在查看的节点：它的讲解任务优先，之前查看的节点转为后台继续。
    pub fn focus_node(&self, r: NodeRef) {
        let mut focus = lock(&self.inner.focus);
        if let Some((s, n)) = focus.take()
            && (s.as_str(), n) != (r.study_id.as_str(), r.node_id)
        {
            self.inner.engine.reprioritize(&explain_group(&s, n), Priority::Background);
        }
        self.inner.engine.reprioritize(&explain_group(&r.study_id, r.node_id), Priority::Interactive);
        *focus = Some((r.study_id, r.node_id));
    }

    /// 返回已有讲解，并在需要时排队生成更详细的讲解。
    pub fn explain_move(&self, req: ExplainRequest) -> ApiResult<Option<ExplanationRecord>> {
        let record = self.inner.store.load_explanation(&req.study_id, req.node_id)?;
        // 是否需要重新分析（设置是否改变、是否要求重试）由讲解线程判断
        self.inner.send_explain(ExplainMsg::Task(ExplainTask {
            study_id: req.study_id,
            node_id: req.node_id,
            budget: req.budget,
            priority: Priority::Interactive,
            detailed: true,
            retry: req.retry,
        }));
        Ok(record)
    }

    pub fn analyze_position(&self, req: AnalyzeRequest) -> ApiResult<()> { self.inner.analyze_position(req) }

    /// 对比同一局面下的两种走法；需要搜索时先返回规则事实，完成后推送 `comparison_updated`。
    pub fn compare_moves(&self, req: crate::compare::CompareRequest) -> ApiResult<crate::compare::CompareView> {
        self.inner.compare_moves(req)
    }

    /// 把一着加入复习（见 `review`）。已经在复习中时返回原来的卡片。
    pub fn add_review(&self, r: NodeRef) -> ApiResult<ReviewItem> { self.inner.add_review(&r.study_id, r.node_id) }

    pub fn list_reviews(&self) -> ApiResult<Vec<ReviewItem>> { self.inner.list_reviews() }

    pub fn review_detail(&self, r: ReviewRef) -> ApiResult<ReviewDetail> { self.inner.review_detail(&r.id) }

    pub fn review_attempt(&self, req: ReviewAttemptRequest) -> ApiResult<ReviewItem> {
        self.inner.review_attempt(&req.id, req.choice)
    }

    pub fn update_review(&self, req: ReviewUpdateRequest) -> ApiResult<ReviewItem> { self.inner.update_review(req) }

    pub fn delete_review(&self, r: ReviewRef) -> ApiResult<()> { self.inner.delete_review(&r.id) }

    pub fn cancel_analysis(&self, request_id: &str) { self.inner.engine.cancel_group(&format!("analyze:{request_id}")); }
}

pub(crate) fn side_short(side: Color) -> &'static str {
    match side {
        Color::Red => "红",
        Color::Black => "黑",
    }
}

impl Inner {
    pub(crate) fn config_snapshot(&self) -> AppConfig { self.config.read().map(|c| c.clone()).unwrap_or_default() }

    pub(crate) fn notice(&self, message: String) {
        warn!("{message}");
        let mut notices = lock(&self.notices);
        notices.push_back(message);
        while notices.len() > 30 {
            notices.pop_front();
        }
    }

    /// 在会话锁内修改会话。修改在副本上进行：闭包成功且写入数据库后才替换缓存，
    /// 任何一步失败时内存与数据库都保持原样。
    pub(crate) fn with_study<R>(&self, id: &str, f: impl FnOnce(&mut Study) -> ApiResult<R>) -> ApiResult<R> {
        let mut studies = lock(&self.studies);
        let current = match studies.get(id) {
            Some(s) => s.clone(),
            None => self.store.load_study(id)?.ok_or_else(|| ApiError::from(format!("找不到学习会话 {id}")))?,
        };
        let mut draft = current.clone();
        let result = f(&mut draft)?;
        if draft.revision != current.revision {
            self.store.save_study(&draft)?;
            studies.insert(id.to_string(), draft);
        } else if !studies.contains_key(id) {
            studies.insert(id.to_string(), current);
        }
        Ok(result)
    }

    pub(crate) fn new_study(
        &self, name: String, source: StudySource, start: StudyStart, orientation: Option<Orientation>,
        user_side: Option<Color>, moves: &[xq_core::Move],
    ) -> ApiResult<StudyView> {
        let StudyStart { pos, counters, history_complete } = start;
        let user_side = user_side.unwrap_or(if pos.is_startpos() { Color::Red } else { pos.side });
        let orientation = orientation.unwrap_or(match user_side {
            Color::Red => Orientation::RedBottom,
            Color::Black => Orientation::BlackBottom,
        });
        let mut study = Study::new(NewStudy {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            source,
            start: pos,
            counters,
            history_complete,
            orientation,
            user_side,
            rules: self.config_snapshot().rules(),
            now: now_ms(),
        })?;
        let mut cur = ROOT;
        let mut created = Vec::new();
        for mv in moves {
            let outcome = study.apply_move(cur, *mv, MoveSource::Record, now_ms())?;
            created.push(outcome.node);
            cur = outcome.node;
        }
        self.store.save_study(&study)?;
        lock(&self.studies).insert(study.id.clone(), study.clone());
        self.on_new_nodes(&study, &created, "create", Priority::Background);
        self.study_view(&study)
    }

    /// 新记录的走子：立即排队生成基本讲解，并按设置排队详细讲解。
    pub(crate) fn on_new_nodes(&self, study: &Study, nodes: &[NodeId], reason: &str, priority: Priority) {
        let auto = self.config_snapshot().analysis.auto_explain;
        for node in nodes {
            if study.nodes.get(node).is_some_and(|n| n.is_move()) {
                let mut record = ExplanationRecord::new(&study.id, *node, now_ms());
                record.pending = true;
                if let Err(e) = self.store.save_explanation(&record) {
                    error!("保存讲解任务失败：{e}");
                }
                self.enqueue(&study.id, *node, Budget::Quick, priority, auto);
            }
        }
        self.emit_study_updated(study, reason, nodes);
    }

    pub(crate) fn enqueue(&self, study_id: &str, node_id: NodeId, budget: Budget, priority: Priority, detailed: bool) {
        let task = ExplainTask { study_id: study_id.to_string(), node_id, budget, priority, detailed, retry: false };
        self.send_explain(ExplainMsg::Task(task));
    }

    /// 在讲解线程上继续处理：引擎任务与 AI 讲解请求的回调、AI 讲解请求的登记。
    pub(crate) fn on_explain_thread(&self, step: impl FnOnce(&Inner) + Send + 'static) {
        self.send_explain(ExplainMsg::Continue(Box::new(step)));
    }

    fn send_explain(&self, msg: ExplainMsg) {
        if lock(&self.explain_tx).send(msg).is_err() {
            error!("讲解线程已停止");
        }
    }

    pub(crate) fn emit_study_updated(&self, study: &Study, reason: &str, new_nodes: &[NodeId]) {
        #[derive(Serialize)]
        struct Payload<'a> {
            study_id: &'a str,
            revision: u64,
            reason: &'a str,
            new_nodes: &'a [NodeId],
            live_node: Option<NodeId>,
        }
        events::emit(self.events.as_ref(), events::STUDY_UPDATED, &Payload {
            study_id: &study.id,
            revision: study.revision,
            reason,
            new_nodes,
            live_node: study.live_node,
        });
    }

    pub(crate) fn study_view(&self, study: &Study) -> ApiResult<StudyView> {
        // 先取序号再读记录：读取期间若有新记录，视图的序号偏小，界面会重新获取
        let explain_seq = self.explain_seq.load(Ordering::SeqCst);
        let records = self.store.explanations_for(&study.id)?;
        let mut explain_status = BTreeMap::new();
        let mut stats = ExplainStats { moves: study.move_nodes().count(), ..Default::default() };
        for r in records {
            if !study.nodes.contains_key(&r.node_id) {
                continue;
            }
            let status = r.status();
            if r.pending {
                stats.pending += 1;
            }
            if r.explanation.is_some() {
                stats.basic += 1;
            }
            match status {
                ExplainStatus::Detailed => stats.detailed += 1,
                ExplainStatus::Partial => stats.partial += 1,
                ExplainStatus::Failed => stats.failed += 1,
                _ => {}
            }
            explain_status.insert(r.node_id, status);
        }
        Ok(StudyView { study: study.clone(), explain_status, stats, explain_seq })
    }

    pub(crate) fn explain_stats(&self, study_id: &str) -> ExplainStats {
        let study = lock(&self.studies).get(study_id).cloned();
        match study {
            Some(s) => self.study_view(&s).map(|v| v.stats).unwrap_or_default(),
            None => ExplainStats::default(),
        }
    }

    pub(crate) fn context_at(&self, study: &Study, node: NodeId) -> ApiResult<ExplainContext> {
        Ok(ExplainContext {
            history_complete: study.history_complete_at(node)?,
            repetition_count: study.repetition_count(node)?,
        })
    }

    fn node_detail(&self, study_id: &str, node_id: NodeId) -> ApiResult<NodeDetail> {
        let study = self.with_study(study_id, |s| Ok(s.clone()))?;
        let node = study.node(node_id)?.clone();
        let pos = study.position(node_id)?;
        let ctx = self.context_at(&study, node_id)?;
        let engine = self.cached_position_evidence(&study, node_id);
        let position = xq_core::explain::explain_position(&pos, &ctx, engine.as_ref());
        let hints = xq_core::explain::hints::think_hints(&pos, engine.as_ref());
        let explanation = if node.is_move() { self.store.load_explanation(study_id, node_id)? } else { None };
        let coach = node.is_move().then(|| self.coach_view(study_id, node_id));
        let review = if node.is_move() { self.review_status(study_id, node_id) } else { None };
        Ok(NodeDetail {
            study_id: study_id.to_string(),
            revision: study.revision,
            legal_moves: rules::legal_moves(&pos),
            status: rules::game_status(&pos),
            in_check: rules::in_check(&pos.board, pos.side),
            history_complete: ctx.history_complete,
            repetition: ctx.repetition_count,
            cloud: self.store.load_cloud(&node.fen).ok().flatten(),
            node,
            position,
            explanation,
            coach,
            hints,
            review,
        })
    }
}
