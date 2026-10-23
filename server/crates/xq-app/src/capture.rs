//! 屏幕连接流程：定位 → 确认起始局面 → 持续识子并用合法走法确认变化。
//!
//! 截图与识别由 `CaptureBackend` 提供（Tauri 程序里是 xcap + ONNX 模型，开发桥里是模拟输入）。
//! 识别结果在被确认之前只是观察数据，不会直接改写棋局。

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use xq_core::Board;
use xq_core::Color;
use xq_core::Move;
use xq_core::Position;
use xq_core::Square;
use xq_core::capture;
use xq_core::capture::CaptureTracker;
use xq_core::capture::CellReading;
use xq_core::capture::Observation;
use xq_core::capture::Rect;
use xq_core::capture::TrackEvent;
use xq_core::capture::TrackerConfig;
use xq_core::fen;
use xq_core::study::NodeId;
use xq_core::study::Orientation;
use xq_core::validate;

use crate::config::CaptureConfig;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub window: WindowInfo,
    pub width: u32,
    pub height: u32,
    /// PNG 的 data URL；没有截图时为空。
    pub image: Option<String>,
    /// 自动定位到的棋盘区域（截图坐标）。
    pub suggested: Option<Rect>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecognitionStatus {
    pub available: bool,
    pub model: String,
    pub detail: String,
}

pub struct Frame {
    pub observation: Observation,
}

#[derive(Clone, Debug)]
pub enum FrameError {
    WindowGone(String),
    Resized(String),
    Capture(String),
    Recognition(String),
}

pub trait FrameSource: Send {
    fn grab(&mut self) -> Result<Frame, FrameError>;
}

pub trait CaptureBackend: Send + Sync {
    fn status(&self) -> RecognitionStatus;
    fn list_windows(&self) -> Result<Vec<WindowInfo>, String>;
    fn snapshot(&self, window_id: u32) -> Result<Snapshot, String>;
    /// 打开持续截图；`rect` 为截图坐标下的棋盘区域（按 9×10 等分后格心落在交叉点上）。
    fn open(&self, window_id: u32, rect: Rect) -> Result<Box<dyn FrameSource>, String>;
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObservedBoard {
    /// 标准方向（红方在下）的棋子布局 FEN，不含行棋方。
    pub board: String,
    pub orientation: Option<Orientation>,
    pub uncertain: Vec<Square>,
    pub issues: Vec<String>,
    /// 与上一个已确认局面不同的点。
    pub changed: Vec<Square>,
    pub suggested_side: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationKind {
    Unexplained,
    NewGame,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptureState {
    Idle,
    Locating {
        window: WindowInfo,
        message: Option<String>,
    },
    AwaitingStart {
        window: WindowInfo,
        observed: ObservedBoard,
    },
    Tracking {
        window: WindowInfo,
        study_id: String,
        node_id: NodeId,
        message: Option<String>,
    },
    AwaitingConfirmation {
        window: WindowInfo,
        study_id: String,
        node_id: NodeId,
        observed: ObservedBoard,
        kind: ConfirmationKind,
        reason: String,
    },
    Stopped {
        window: Option<WindowInfo>,
        stage: String,
        message: String,
    },
}

pub trait CaptureHost: Send + Sync {
    /// 把确认的走法记入棋局，返回实际对局的最新节点。
    fn record_capture_moves(&self, study_id: &str, from: NodeId, moves: &[Move]) -> Result<NodeId, String>;
    fn capture_state_changed(&self, state: &CaptureState);
}

struct Pending {
    observed: ObservedBoard,
    kind: ConfirmationKind,
    reason: String,
}

struct Tracking {
    study_id: String,
    node_id: NodeId,
    orientation: Orientation,
    tracker: CaptureTracker,
    pending: Option<Pending>,
}

enum Phase {
    Locating { last: Option<Board>, count: u32 },
    AwaitingStart(ObservedBoard),
    Tracking(Box<Tracking>),
    Stopped { stage: String, message: String },
}

struct Shared {
    window: WindowInfo,
    cfg: CaptureConfig,
    phase: Phase,
    message: Option<String>,
}

impl Shared {
    fn state(&self) -> CaptureState {
        let window = self.window.clone();
        match &self.phase {
            Phase::Locating { .. } => CaptureState::Locating { window, message: self.message.clone() },
            Phase::AwaitingStart(observed) => CaptureState::AwaitingStart { window, observed: observed.clone() },
            Phase::Tracking(t) => match &t.pending {
                Some(p) => CaptureState::AwaitingConfirmation {
                    window,
                    study_id: t.study_id.clone(),
                    node_id: t.node_id,
                    observed: p.observed.clone(),
                    kind: p.kind,
                    reason: p.reason.clone(),
                },
                None => CaptureState::Tracking {
                    window,
                    study_id: t.study_id.clone(),
                    node_id: t.node_id,
                    message: self.message.clone(),
                },
            },
            Phase::Stopped { stage, message } => {
                CaptureState::Stopped { window: Some(window), stage: stage.clone(), message: message.clone() }
            }
        }
    }

    fn tracker_config(&self) -> TrackerConfig {
        TrackerConfig { stable_frames: self.cfg.stable_frames, unexplained_frames: self.cfg.unexplained_frames }
    }
}

fn observed(board: &Board, orientation: Option<Orientation>, obs: Option<&Observation>, threshold: f32) -> ObservedBoard {
    ObservedBoard {
        board: fen::board_fen(board),
        orientation,
        uncertain: obs
            .map(|o| capture::uncertain_squares(o, orientation.unwrap_or(Orientation::RedBottom), threshold))
            .unwrap_or_default(),
        issues: validate::validate_board(board).into_iter().map(|i| i.message).collect(),
        changed: Vec::new(),
        suggested_side: (*board == Board::startpos()).then_some(Color::Red),
    }
}

/// 截图线程处理一帧后需要在释放锁之后执行的动作。
enum Action {
    None,
    Notify,
    Record { study_id: String, from: NodeId, moves: Vec<Move> },
    Stop,
}

pub struct CaptureSession {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl CaptureSession {
    pub fn start(
        mut source: Box<dyn FrameSource>, window: WindowInfo, cfg: CaptureConfig, host: Weak<dyn CaptureHost>,
    ) -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(Shared {
            window,
            cfg: cfg.clone(),
            phase: Phase::Locating { last: None, count: 0 },
            message: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_shared, thread_stop) = (shared.clone(), stop.clone());
        let interval = Duration::from_millis(cfg.interval_ms);
        let handle = thread::Builder::new()
            .name("capture".into())
            .spawn(move || {
                let mut errors = 0u32;
                while !thread_stop.load(Ordering::Relaxed) {
                    thread::sleep(interval);
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let frame = source.grab();
                    let action = match thread_shared.lock() {
                        Ok(mut shared) => process(&mut shared, frame, &mut errors),
                        Err(_) => Action::Stop,
                    };
                    let Some(host) = host.upgrade() else { break };
                    match action {
                        Action::None => {}
                        Action::Notify => notify(&thread_shared, host.as_ref()),
                        Action::Stop => {
                            notify(&thread_shared, host.as_ref());
                            break;
                        }
                        Action::Record { study_id, from, moves } => {
                            let result = host.record_capture_moves(&study_id, from, &moves);
                            if let Ok(mut shared) = thread_shared.lock() {
                                match (result, &mut shared.phase) {
                                    (Ok(node), Phase::Tracking(t)) => t.node_id = node,
                                    (Err(e), _) => {
                                        shared.phase = Phase::Stopped { stage: "record".into(), message: e };
                                    }
                                    _ => {}
                                }
                            }
                            notify(&thread_shared, host.as_ref());
                        }
                    }
                }
            })
            .map_err(|e| format!("无法启动截图线程：{e}"))?;
        Ok(Self { shared, stop, handle: Some(handle) })
    }

    pub fn state(&self) -> CaptureState {
        self.shared.lock().map(|s| s.state()).unwrap_or(CaptureState::Idle)
    }

    pub fn is_running(&self) -> bool { self.handle.as_ref().is_some_and(|h| !h.is_finished()) }

    /// 用户确认了起始局面：开始按合法走法跟踪。
    pub fn begin_tracking(&self, study_id: &str, node_id: NodeId, pos: Position, orientation: Orientation) -> CaptureState {
        let Ok(mut s) = self.shared.lock() else { return CaptureState::Idle };
        let cfg = s.tracker_config();
        s.message = None;
        s.phase = Phase::Tracking(Box::new(Tracking {
            study_id: study_id.to_string(),
            node_id,
            orientation,
            tracker: CaptureTracker::new(pos, cfg),
            pending: None,
        }));
        s.state()
    }

    /// 用户确认了无法解释的画面（或新一局）：从新的局面与节点继续。
    pub fn accept(&self, study_id: &str, node_id: NodeId, pos: Position) -> CaptureState {
        let Ok(mut s) = self.shared.lock() else { return CaptureState::Idle };
        if let Phase::Tracking(t) = &mut s.phase {
            t.study_id = study_id.to_string();
            t.node_id = node_id;
            t.tracker.reset(pos);
            t.pending = None;
        }
        s.state()
    }

    /// 暂不处理确认请求，继续等待画面自行恢复。
    pub fn dismiss(&self) -> CaptureState {
        let Ok(mut s) = self.shared.lock() else { return CaptureState::Idle };
        if let Phase::Tracking(t) = &mut s.phase {
            t.pending = None;
            t.tracker.resume();
        }
        s.state()
    }

    /// 重新定位起始局面（例如用户想换一个窗口区域后重来）。
    pub fn relocate(&self) -> CaptureState {
        let Ok(mut s) = self.shared.lock() else { return CaptureState::Idle };
        s.phase = Phase::Locating { last: None, count: 0 };
        s.state()
    }

    pub fn tracking_target(&self) -> Option<(String, NodeId, Orientation)> {
        let s = self.shared.lock().ok()?;
        match &s.phase {
            Phase::Tracking(t) => Some((t.study_id.clone(), t.node_id, t.orientation)),
            _ => None,
        }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn notify(shared: &Arc<Mutex<Shared>>, host: &dyn CaptureHost) {
    let state = shared.lock().map(|s| s.state());
    if let Ok(state) = state {
        host.capture_state_changed(&state);
    }
}

fn process(shared: &mut Shared, frame: Result<Frame, FrameError>, errors: &mut u32) -> Action {
    let obs = match frame {
        Ok(f) => {
            *errors = 0;
            f.observation
        }
        Err(FrameError::WindowGone(m)) => {
            shared.phase = Phase::Stopped { stage: "window".into(), message: m };
            return Action::Stop;
        }
        Err(FrameError::Resized(m)) => {
            shared.phase = Phase::Stopped { stage: "calibration".into(), message: m };
            return Action::Stop;
        }
        Err(FrameError::Capture(m) | FrameError::Recognition(m)) => {
            *errors += 1;
            if *errors >= 40 {
                shared.phase = Phase::Stopped { stage: "capture".into(), message: format!("连续多次截图或识别失败：{m}") };
                return Action::Stop;
            }
            let changed = shared.message.as_deref() != Some(m.as_str());
            shared.message = Some(m);
            return if changed { Action::Notify } else { Action::None };
        }
    };
    if !obs.board_found {
        let m = "画面中未识别到棋盘，可能被遮挡或窗口布局改变".to_string();
        let changed = shared.message.as_deref() != Some(m.as_str());
        shared.message = Some(m);
        return if changed { Action::Notify } else { Action::None };
    }
    let had_message = shared.message.take().is_some();
    let threshold = shared.cfg.confidence_threshold;
    let stable_frames = shared.cfg.stable_frames;
    match &mut shared.phase {
        Phase::Locating { last, count } => {
            let raw = capture::to_board(&obs, Orientation::RedBottom);
            if last.as_ref() == Some(&raw) {
                *count += 1;
            } else {
                *last = Some(raw);
                *count = 1;
            }
            if *count < stable_frames {
                return if had_message { Action::Notify } else { Action::None };
            }
            let orientation = capture::infer_orientation(&obs);
            let board = capture::to_board(&obs, orientation.unwrap_or(Orientation::RedBottom));
            shared.phase = Phase::AwaitingStart(observed(&board, orientation, Some(&obs), threshold));
            Action::Notify
        }
        Phase::AwaitingStart(_) | Phase::Stopped { .. } => Action::None,
        Phase::Tracking(t) => {
            let board = capture::to_board(&obs, t.orientation);
            match t.tracker.feed(board) {
                TrackEvent::Moves(moves) => {
                    t.pending = None;
                    Action::Record { study_id: t.study_id.clone(), from: t.node_id, moves }
                }
                TrackEvent::NewGame => {
                    let mut seen = observed(&Board::startpos(), Some(t.orientation), Some(&obs), threshold);
                    seen.suggested_side = Some(Color::Red);
                    t.pending = Some(Pending {
                        observed: seen,
                        kind: ConfirmationKind::NewGame,
                        reason: "画面回到了开局，是否作为新的一局开始记录？".into(),
                    });
                    Action::Notify
                }
                TrackEvent::NeedsConfirmation { board, reason } => {
                    let mut seen = observed(&board, Some(t.orientation), Some(&obs), threshold);
                    let confirmed = &t.tracker.confirmed().board;
                    seen.changed = Square::all().filter(|sq| confirmed.get(*sq) != board.get(*sq)).collect();
                    t.pending = Some(Pending { observed: seen, kind: ConfirmationKind::Unexplained, reason });
                    Action::Notify
                }
                TrackEvent::Idle => {
                    if had_message {
                        Action::Notify
                    } else {
                        Action::None
                    }
                }
            }
        }
    }
}

/// 开发与测试用的模拟输入：画面内容通过 `show` 设置。
#[derive(Default)]
pub struct SimBackend {
    screen: Arc<Mutex<Option<Board>>>,
}

impl SimBackend {
    pub fn new() -> Self { Self::default() }

    /// 设置模拟画面。`orientation` 为黑方在下时，画面是标准棋盘旋转 180°。
    pub fn show(&self, board: Option<Board>, orientation: Orientation) {
        let screen = board.map(|b| match orientation {
            Orientation::RedBottom => b,
            Orientation::BlackBottom => b.rotated(),
        });
        if let Ok(mut s) = self.screen.lock() {
            *s = screen;
        }
    }

    fn window() -> WindowInfo {
        WindowInfo { id: 1, title: "模拟对弈窗口".into(), app_name: "xq-devserver".into(), width: 900, height: 1000 }
    }
}

impl CaptureBackend for SimBackend {
    fn status(&self) -> RecognitionStatus {
        RecognitionStatus {
            available: true,
            model: "模拟输入".into(),
            detail: "开发用：画面内容由测试接口设置，不使用识别模型".into(),
        }
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, String> { Ok(vec![Self::window()]) }

    fn snapshot(&self, window_id: u32) -> Result<Snapshot, String> {
        if window_id != 1 {
            return Err("窗口不存在".into());
        }
        Ok(Snapshot {
            window: Self::window(),
            width: 900,
            height: 1000,
            image: None,
            suggested: Some(Rect { x: 50.0, y: 50.0, w: 800.0, h: 900.0 }),
            note: Some("模拟窗口没有截图，直接确认区域即可".into()),
        })
    }

    fn open(&self, window_id: u32, _rect: Rect) -> Result<Box<dyn FrameSource>, String> {
        if window_id != 1 {
            return Err("窗口不存在".into());
        }
        Ok(Box::new(SimSource { screen: self.screen.clone() }))
    }
}

struct SimSource {
    screen: Arc<Mutex<Option<Board>>>,
}

impl FrameSource for SimSource {
    fn grab(&mut self) -> Result<Frame, FrameError> {
        let screen = self.screen.lock().map_err(|_| FrameError::Capture("模拟画面不可用".into()))?.clone();
        let Some(board) = screen else { return Err(FrameError::Recognition("模拟画面为空".into())) };
        let mut cells = [[CellReading::default(); 9]; 10];
        for (sq, piece) in board.pieces() {
            cells[9 - sq.rank() as usize][sq.file() as usize] =
                CellReading { piece: Some(piece), confidence: 0.97, conflict: false };
        }
        Ok(Frame { observation: Observation { cells, board_found: true } })
    }
}
