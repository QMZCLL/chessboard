use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use tracing::info;
use tracing::warn;
use xq_core::Position;
use xq_core::explain::EngineLine;
use xq_core::fen;
use xq_core::rules;
use xq_core::rules::GameStatus;
use xq_core::uci;
use xq_core::uci::BestMove;
use xq_core::uci::Bound;
use xq_core::uci::UciOption;

use super::AppliedOption;
use super::EngineIdentity;
use super::EngineSpec;
use super::EngineState;
use super::JobId;
use super::Priority;
use super::SearchOutcome;
use super::SearchProgress;
use super::SearchRequest;
use super::SearchResult;
use super::process::EngineProcess;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// 超过搜索时间后再等待多久发送 stop。
const DEADLINE_GRACE: Duration = Duration::from_secs(5);
/// 只限深度时的最长搜索时间。
const DEPTH_ONLY_CAP: Duration = Duration::from_secs(120);
/// 发送 stop 后多久仍无 bestmove 就结束进程。
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

type ProgressFn = Box<dyn FnMut(SearchProgress) + Send>;
type DoneFn = Box<dyn FnOnce(SearchOutcome) + Send>;
type StateFn = Box<dyn Fn(&EngineState) + Send>;

/// 调用任务回调；回调中的错误只影响该任务，不会让引擎服务线程退出。
fn run_callback(what: &str, f: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
        tracing::error!("引擎任务的{what}回调出错，已跳过");
    }
}

fn finish_job(job: Job, outcome: SearchOutcome) {
    let done = job.spec.on_done;
    run_callback("完成", move || done(outcome));
}

pub struct JobSpec {
    pub priority: Priority,
    /// 任务分组（如某个节点的讲解），用于整体取消或调整优先级。
    pub group: String,
    pub request: SearchRequest,
    pub on_progress: Option<ProgressFn>,
    pub on_done: DoneFn,
}

pub(super) struct Job {
    id: JobId,
    seq: u64,
    spec: JobSpec,
}

pub(super) enum Msg {
    Submit(Job),
    Cancel(JobId),
    CancelGroup(String),
    Reprioritize { group: String, priority: Priority },
    Reconfigure(EngineSpec),
    Restart,
    Shutdown,
    Line(u64, String),
    Eof(u64),
}

pub struct EngineService {
    tx: Sender<Msg>,
    state: Arc<Mutex<EngineState>>,
    next_id: AtomicU64,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl EngineService {
    pub fn start(spec: EngineSpec, on_state: StateFn) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(EngineState::Starting));
        let worker = Worker::new(spec, rx, tx.clone(), state.clone(), on_state);
        let handle = thread::Builder::new()
            .name("engine-service".into())
            .spawn(move || worker.run())
            .map_err(|e| format!("无法启动引擎服务线程：{e}"))?;
        Ok(Self { tx, state, next_id: AtomicU64::new(1), worker: Mutex::new(Some(handle)) })
    }

    pub fn state(&self) -> EngineState { self.state.lock().map(|s| s.clone()).unwrap_or(EngineState::Stopped) }

    /// 提交搜索任务；服务已停止时立即以失败结束。
    pub fn submit(&self, spec: JobSpec) -> JobId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Err(mpsc::SendError(Msg::Submit(job))) = self.tx.send(Msg::Submit(Job { id, seq: id, spec })) {
            finish_job(job, SearchOutcome::Failed("引擎服务已停止".into()));
        }
        id
    }

    pub fn cancel(&self, job: JobId) { let _ = self.tx.send(Msg::Cancel(job)); }

    pub fn cancel_group(&self, group: &str) { let _ = self.tx.send(Msg::CancelGroup(group.to_string())); }

    pub fn reprioritize(&self, group: &str, priority: Priority) {
        let _ = self.tx.send(Msg::Reprioritize { group: group.to_string(), priority });
    }

    /// 更新配置；在引擎空闲时生效，引擎文件变化时重启进程。
    pub fn reconfigure(&self, spec: EngineSpec) { let _ = self.tx.send(Msg::Reconfigure(spec)); }

    pub fn restart(&self) { let _ = self.tx.send(Msg::Restart); }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(handle) = self.worker.lock().ok().and_then(|mut w| w.take()) {
            let _ = handle.join();
        }
    }
}

impl Drop for EngineService {
    fn drop(&mut self) { self.shutdown(); }
}

struct Running {
    job: Job,
    lines: BTreeMap<u32, EngineLine>,
    depth: u32,
    nodes: u64,
    time_ms: u64,
    deadline: Instant,
    stop_sent: Option<Instant>,
    cancelled: bool,
    preempted: bool,
    last_progress: Instant,
}

struct Worker {
    spec: EngineSpec,
    pending_spec: Option<EngineSpec>,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    state: Arc<Mutex<EngineState>>,
    on_state: StateFn,
    process: Option<EngineProcess>,
    generation: u64,
    options: Vec<UciOption>,
    identity: Option<EngineIdentity>,
    applied: Vec<AppliedOption>,
    notes: Vec<String>,
    current_multipv: u32,
    queue: Vec<Job>,
    running: Option<Running>,
    deferred: VecDeque<Msg>,
    crashes: Vec<Instant>,
    failure: Option<String>,
}

impl Worker {
    fn new(spec: EngineSpec, rx: Receiver<Msg>, tx: Sender<Msg>, state: Arc<Mutex<EngineState>>, on_state: StateFn) -> Self {
        Self {
            spec,
            pending_spec: None,
            rx,
            tx,
            state,
            on_state,
            process: None,
            generation: 0,
            options: Vec::new(),
            identity: None,
            applied: Vec::new(),
            notes: Vec::new(),
            current_multipv: 1,
            queue: Vec::new(),
            running: None,
            deferred: VecDeque::new(),
            crashes: Vec::new(),
            failure: None,
        }
    }

    fn set_state(&self, state: EngineState) {
        (self.on_state)(&state);
        if let Ok(mut s) = self.state.lock() {
            *s = state;
        }
    }

    fn run(mut self) {
        self.boot();
        loop {
            let msg = match self.deferred.pop_front() {
                Some(m) => Some(m),
                None => match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
            };
            if let Some(msg) = msg
                && !self.handle(msg)
            {
                break;
            }
            self.check_timeouts();
            self.start_next();
        }
        if let Some(mut p) = self.process.take() {
            p.quit();
        }
        self.set_state(EngineState::Stopped);
    }

    /// 处理一条消息；返回 false 表示服务应退出。
    fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Submit(job) => {
                let interactive = job.spec.priority == Priority::Interactive;
                self.queue.push(job);
                if interactive {
                    self.preempt_background();
                }
            }
            Msg::Cancel(id) => self.cancel_where(|job| job.id == id),
            Msg::CancelGroup(group) => self.cancel_where(|job| job.spec.group == group),
            Msg::Reprioritize { group, priority } => {
                for job in self.queue.iter_mut().filter(|j| j.spec.group == group) {
                    job.spec.priority = priority;
                }
                if let Some(r) = self.running.as_mut().filter(|r| r.job.spec.group == group) {
                    r.job.spec.priority = priority;
                }
                if self.queue.iter().any(|j| j.spec.priority == Priority::Interactive) {
                    self.preempt_background();
                }
            }
            Msg::Reconfigure(spec) => self.pending_spec = Some(spec),
            Msg::Restart => {
                if let Some(r) = self.running.take() {
                    self.queue.push(r.job);
                }
                self.kill_process();
                self.crashes.clear();
                self.boot();
            }
            Msg::Shutdown => {
                if let Some(r) = self.running.take() {
                    finish_job(r.job, SearchOutcome::Cancelled);
                }
                for job in self.queue.drain(..) {
                    finish_job(job, SearchOutcome::Cancelled);
                }
                return false;
            }
            Msg::Line(generation, line) if generation == self.generation => self.on_line(&line),
            Msg::Eof(generation) if generation == self.generation => self.on_crash("引擎进程意外退出"),
            Msg::Line(..) | Msg::Eof(_) => {}
        }
        true
    }

    fn send(&mut self, command: &str) -> bool {
        match self.process.as_mut() {
            Some(p) => p.send(command).is_ok(),
            None => false,
        }
    }

    /// 搜索期间的命令：发送失败说明进程已不可用，按崩溃处理。
    fn send_or_crash(&mut self, command: &str) {
        if !self.send(command) {
            self.on_crash("无法向引擎发送命令");
        }
    }

    fn preempt_background(&mut self) {
        let mut stop = false;
        if let Some(r) = self.running.as_mut()
            && r.job.spec.priority == Priority::Background
            && !r.cancelled
            && !r.preempted
        {
            r.preempted = true;
            r.stop_sent = Some(Instant::now());
            stop = true;
        }
        if stop {
            self.send_or_crash("stop");
        }
    }

    fn cancel_where(&mut self, pred: impl Fn(&Job) -> bool) {
        let (cancelled, kept): (Vec<Job>, Vec<Job>) = self.queue.drain(..).partition(|j| pred(j));
        self.queue = kept;
        for job in cancelled {
            finish_job(job, SearchOutcome::Cancelled);
        }
        let mut stop = false;
        if let Some(r) = self.running.as_mut()
            && pred(&r.job)
            && !r.cancelled
        {
            r.cancelled = true;
            r.preempted = false;
            if r.stop_sent.is_none() {
                r.stop_sent = Some(Instant::now());
                stop = true;
            }
        }
        if stop {
            self.send_or_crash("stop");
        }
    }

    fn on_line(&mut self, line: &str) {
        let Some(r) = self.running.as_mut() else { return };
        if let Some(info) = uci::parse_info(line) {
            if !info.is_search_line() || info.pv.is_empty() {
                return;
            }
            let depth = info.depth.unwrap_or(0);
            let deeper = depth > r.depth;
            r.lines.insert(info.multipv.unwrap_or(1), EngineLine {
                mv: info.pv[0],
                score: info.score.expect("is_search_line 已确认有评分"),
                bound: info.bound.unwrap_or(Bound::Exact),
                depth,
                pv: info.pv,
            });
            r.depth = r.depth.max(depth);
            r.nodes = info.nodes.unwrap_or(r.nodes);
            r.time_ms = info.time_ms.unwrap_or(r.time_ms);
            if (deeper || r.last_progress.elapsed() >= PROGRESS_INTERVAL)
                && let Some(progress) = r.job.spec.on_progress.as_mut()
            {
                r.last_progress = Instant::now();
                let update = SearchProgress {
                    job: r.job.id,
                    depth: r.depth,
                    nodes: r.nodes,
                    time_ms: r.time_ms,
                    lines: r.lines.values().cloned().collect(),
                };
                run_callback("进度", || progress(update));
            }
        } else if let Some(best) = uci::parse_bestmove(line) {
            self.finish(best);
        }
    }

    fn finish(&mut self, best: BestMove) {
        let Some(r) = self.running.take() else { return };
        if r.preempted {
            self.queue.push(r.job);
            return;
        }
        if r.cancelled {
            finish_job(r.job, SearchOutcome::Cancelled);
            return;
        }
        let outcome = match best {
            BestMove::None => match request_position(&r.job.spec.request) {
                Ok(pos) if rules::game_status(&pos) != GameStatus::Ongoing => {
                    SearchOutcome::Terminal(rules::game_status(&pos))
                }
                _ => SearchOutcome::Failed("引擎没有给出着法".into()),
            },
            BestMove::Move { mv, .. } if !r.lines.is_empty() => SearchOutcome::Completed(SearchResult {
                lines: r.lines.into_values().collect(),
                bestmove: Some(mv),
                depth: r.depth,
                nodes: r.nodes,
                time_ms: r.time_ms,
                identity: self.identity.clone().unwrap_or_else(unknown_identity),
            }),
            BestMove::Move { .. } => SearchOutcome::Failed("引擎没有返回评分和主变化".into()),
        };
        finish_job(r.job, outcome);
    }

    fn check_timeouts(&mut self) {
        let now = Instant::now();
        let mut send_stop = false;
        let mut kill = false;
        if let Some(r) = self.running.as_mut() {
            match r.stop_sent {
                None if now >= r.deadline => {
                    r.stop_sent = Some(now);
                    send_stop = true;
                }
                Some(t) if now.duration_since(t) > STOP_TIMEOUT => kill = true,
                _ => {}
            }
        }
        if send_stop {
            self.send_or_crash("stop");
        }
        if kill {
            warn!("引擎在 stop 后没有结束搜索，结束进程并重启");
            self.on_crash("引擎超时未响应");
        }
    }

    /// 进程退出或无响应：结束当前任务，必要时自动重启。
    fn on_crash(&mut self, reason: &str) {
        if let Some(r) = self.running.take() {
            if r.preempted {
                self.queue.push(r.job);
            } else if r.cancelled {
                finish_job(r.job, SearchOutcome::Cancelled);
            } else {
                finish_job(r.job, SearchOutcome::Failed(format!("{reason}，本次分析未完成")));
            }
        }
        self.kill_process();
        let now = Instant::now();
        self.crashes.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        self.crashes.push(now);
        if self.crashes.len() <= 2 {
            warn!("{reason}，自动重启引擎");
            self.boot();
        } else {
            let message = format!("{reason}；短时间内多次失败，已停止自动重启，可在界面中手动重启");
            self.fail(message);
        }
    }

    fn fail(&mut self, reason: String) {
        warn!("引擎不可用：{reason}");
        self.failure = Some(reason.clone());
        self.set_state(EngineState::Failed { reason: reason.clone() });
        for job in self.queue.drain(..) {
            finish_job(job, SearchOutcome::Failed(format!("引擎不可用：{reason}")));
        }
    }

    fn kill_process(&mut self) {
        if let Some(mut p) = self.process.take() {
            p.kill();
        }
        // 让旧进程的剩余输出全部失效
        self.generation += 1;
    }

    /// 等待满足条件的引擎输出行；期间收到的其他消息延后处理。
    fn wait_for(&mut self, timeout: Duration, mut done: impl FnMut(&str) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("等待引擎响应超时".into());
            }
            match self.rx.recv_timeout(left) {
                Ok(Msg::Line(g, line)) if g == self.generation => {
                    if done(&line) {
                        return Ok(());
                    }
                }
                Ok(Msg::Eof(g)) if g == self.generation => return Err("引擎进程已退出".into()),
                Ok(Msg::Line(..) | Msg::Eof(_)) => {}
                Ok(other) => self.deferred.push_back(other),
                Err(RecvTimeoutError::Timeout) => return Err("等待引擎响应超时".into()),
                Err(RecvTimeoutError::Disconnected) => return Err("引擎服务已停止".into()),
            }
        }
    }

    fn boot(&mut self) {
        self.set_state(EngineState::Starting);
        self.kill_process();
        self.failure = None;
        let process = match EngineProcess::spawn(&self.spec.binary, self.generation, self.tx.clone()) {
            Ok(p) => p,
            Err(e) => {
                self.fail(format!("无法启动引擎 {}：{e}", self.spec.binary.display()));
                return;
            }
        };
        self.process = Some(process);
        let mut name = String::from("未知引擎");
        let mut options = Vec::new();
        if !self.send("uci") {
            self.kill_process();
            self.fail("无法向引擎发送命令".into());
            return;
        }
        let handshake = self.wait_for(HANDSHAKE_TIMEOUT, |line| {
            if let Some(n) = line.strip_prefix("id name ") {
                name = n.trim().to_string();
            } else if let Some(o) = uci::parse_option(line) {
                options.push(o);
            }
            line == "uciok"
        });
        if let Err(e) = handshake {
            self.kill_process();
            self.fail(format!("引擎握手失败：{e}"));
            return;
        }
        self.options = options;
        self.applied.clear();
        self.notes.clear();
        let score_type = self.configure();
        if let Err(e) = self.sync() {
            self.kill_process();
            self.fail(format!("引擎初始化失败：{e}"));
            return;
        }
        let nnue = match &self.spec.nnue {
            Some(path) => match std::fs::metadata(path) {
                Ok(meta) => format!(
                    "{}（{:.1} MB）",
                    path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                    meta.len() as f64 / 1_048_576.0
                ),
                Err(_) => format!("{}（文件不存在）", path.display()),
            },
            None => "引擎默认".to_string(),
        };
        let identity = EngineIdentity { name, nnue, score_type, generation: self.generation };
        info!("引擎就绪：{} / {}", identity.name, identity.nnue);
        self.identity = Some(identity.clone());
        self.set_state(EngineState::Ready { identity, applied: self.applied.clone(), notes: self.notes.clone() });
    }

    fn option(&self, name: &str) -> Option<&UciOption> { self.options.iter().find(|o| o.name.eq_ignore_ascii_case(name)) }

    fn set_option(&mut self, name: &str, value: String) {
        // 发送失败会在随后的 isready 同步中暴露
        let _ = self.send(&format!("setoption name {name} value {value}"));
        self.applied.retain(|o| o.name != name);
        self.applied.push(AppliedOption { name: name.to_string(), value });
    }

    fn clamp_spin(&self, name: &str, value: u32) -> u32 {
        match self.option(name) {
            Some(o) => {
                let min = o.min.unwrap_or(1).max(0) as u32;
                let max = o.max.unwrap_or(i64::from(u32::MAX)).min(i64::from(u32::MAX)) as u32;
                value.clamp(min, max)
            }
            None => value,
        }
    }

    /// 只设置引擎报告支持的选项，返回实际使用的评分类型。
    fn configure(&mut self) -> String {
        if let Some(nnue) = self.spec.nnue.clone() {
            if self.option("EvalFile").is_some() {
                self.set_option("EvalFile", nnue.display().to_string());
            } else {
                self.notes.push("引擎不支持 EvalFile，使用其默认网络文件".into());
            }
        }
        for (name, value) in [("Threads", self.spec.threads), ("Hash", self.spec.hash_mb)] {
            if self.option(name).is_some() {
                let clamped = self.clamp_spin(name, value);
                if clamped != value {
                    self.notes.push(format!("{name} 超出引擎允许范围，已调整为 {clamped}"));
                }
                self.set_option(name, clamped.to_string());
            }
        }
        if self.option("MultiPV").is_some() {
            self.set_option("MultiPV", "1".into());
        }
        self.current_multipv = 1;
        let rule = self.spec.rules.repetition_rule.clone();
        match self.option("Repetition Rule").map(|o| o.vars.contains(&rule)) {
            Some(true) => self.set_option("Repetition Rule", rule),
            Some(false) => self.notes.push(format!("引擎不支持重复局面规则 {rule}，使用引擎默认规则")),
            None => self.notes.push("引擎没有提供重复局面规则选项".into()),
        }
        if self.option("Sixty Move Rule").is_some() {
            self.set_option("Sixty Move Rule", self.spec.rules.sixty_move_rule.to_string());
        }
        // 归一化评分便于比较走法差距；不支持时沿用引擎默认刻度
        match self.option("ScoreType").map(|o| o.vars.iter().any(|v| v == "PawnValueNormalized")) {
            Some(true) => {
                self.set_option("ScoreType", "PawnValueNormalized".into());
                "PawnValueNormalized".into()
            }
            _ => self.option("ScoreType").and_then(|o| o.default.clone()).unwrap_or_else(|| "cp".into()),
        }
    }

    fn sync(&mut self) -> Result<(), String> {
        if !self.send("isready") {
            return Err("无法向引擎发送命令".into());
        }
        self.wait_for(READY_TIMEOUT, |line| line == "readyok")
    }

    fn apply_pending_spec(&mut self) {
        let Some(spec) = self.pending_spec.take() else { return };
        if spec == self.spec {
            return;
        }
        let restart = spec.binary != self.spec.binary || spec.nnue != self.spec.nnue || self.process.is_none();
        self.spec = spec;
        if restart {
            self.crashes.clear();
            self.boot();
            return;
        }
        let score_type = self.configure();
        if let Err(e) = self.sync() {
            self.on_crash(&format!("应用新设置失败：{e}"));
            return;
        }
        if let Some(identity) = self.identity.as_mut() {
            identity.score_type = score_type;
        }
        if let Some(identity) = self.identity.clone() {
            self.set_state(EngineState::Ready { identity, applied: self.applied.clone(), notes: self.notes.clone() });
        }
    }

    fn start_next(&mut self) {
        if self.running.is_some() {
            return;
        }
        self.apply_pending_spec();
        while self.running.is_none() {
            let Some(index) = self
                .queue
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.spec.priority.cmp(&b.spec.priority).then(b.seq.cmp(&a.seq)))
                .map(|(i, _)| i)
            else {
                return;
            };
            let job = self.queue.remove(index);
            let pos = match request_position(&job.spec.request) {
                Ok(p) => p,
                Err(e) => {
                    finish_job(job, SearchOutcome::Failed(e));
                    continue;
                }
            };
            let status = rules::game_status(&pos);
            if status != GameStatus::Ongoing {
                finish_job(job, SearchOutcome::Terminal(status));
                continue;
            }
            if self.process.is_none() {
                let reason = self.failure.clone().unwrap_or_else(|| "引擎未启动".into());
                finish_job(job, SearchOutcome::Failed(format!("引擎不可用：{reason}")));
                continue;
            }
            self.launch(job, &pos);
        }
    }

    fn launch(&mut self, job: Job, pos: &Position) {
        let req = &job.spec.request;
        let legal = rules::legal_moves(pos);
        let searchmoves: Vec<String> = req.searchmoves.iter().filter(|m| legal.contains(m)).map(|m| m.iccs()).collect();
        if !req.searchmoves.is_empty() && searchmoves.is_empty() {
            finish_job(job, SearchOutcome::Failed("指定的着法在该局面不合法".into()));
            return;
        }
        let limit = if searchmoves.is_empty() { legal.len() } else { searchmoves.len() };
        let multipv = req.multipv.clamp(1, limit.max(1) as u32);
        if multipv != self.current_multipv && self.option("MultiPV").is_some() {
            self.set_option("MultiPV", multipv.to_string());
            self.current_multipv = multipv;
        }
        let mut position = format!("position fen {}", req.base_fen);
        if !req.moves.is_empty() {
            let moves: Vec<String> = req.moves.iter().map(|m| m.iccs()).collect();
            position.push_str(" moves ");
            position.push_str(&moves.join(" "));
        }
        let mut go = String::from("go");
        if let Some(d) = req.depth {
            go.push_str(&format!(" depth {d}"));
        }
        if let Some(t) = req.movetime_ms {
            go.push_str(&format!(" movetime {t}"));
        }
        // searchmoves 会吞掉其后的所有参数，必须放在最后
        if !searchmoves.is_empty() {
            go.push_str(" searchmoves ");
            go.push_str(&searchmoves.join(" "));
        }
        let budget = req.movetime_ms.map(Duration::from_millis).unwrap_or(DEPTH_ONLY_CAP);
        let now = Instant::now();
        self.running = Some(Running {
            job,
            lines: BTreeMap::new(),
            depth: 0,
            nodes: 0,
            time_ms: 0,
            deadline: now + budget + DEADLINE_GRACE,
            stop_sent: None,
            cancelled: false,
            preempted: false,
            last_progress: now,
        });
        if !(self.send(&position) && self.send(&go)) {
            self.on_crash("无法向引擎发送命令");
        }
    }
}

fn unknown_identity() -> EngineIdentity {
    EngineIdentity { name: "未知引擎".into(), nnue: String::new(), score_type: String::new(), generation: 0 }
}

/// 按请求重放出根局面，同时校验起始局面与历史着法。
pub(crate) fn request_position(req: &SearchRequest) -> Result<Position, String> {
    let mut pos = fen::parse_position(&req.base_fen).map_err(|e| format!("局面无效：{e}"))?;
    for (i, mv) in req.moves.iter().enumerate() {
        if !rules::is_legal(&pos, *mv) {
            return Err(format!("历史第 {} 步 {} 不合法", i + 1, mv));
        }
        pos = pos.after(*mv);
    }
    Ok(pos)
}
