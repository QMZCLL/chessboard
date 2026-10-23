//! 屏幕连接与学习会话之间的衔接。
//!
//! 锁顺序：截图线程先释放自身状态锁再回调服务；服务在持有会话锁时不调用截图会话，避免死锁。

use std::sync::Arc;
use std::sync::Weak;

use xq_core::Move;
use xq_core::Position;
use xq_core::fen;
use xq_core::study::MoveSource;
use xq_core::study::NodeId;
use xq_core::study::ROOT;
use xq_core::study::StudySource;
use xq_core::validate;

use crate::api::ConfirmStartRequest;
use crate::api::ResolveCaptureRequest;
use crate::api::StartCaptureRequest;
use crate::api::StudyView;
use crate::capture::CaptureHost;
use crate::capture::CaptureSession;
use crate::capture::CaptureState;
use crate::capture::Snapshot;
use crate::capture::WindowInfo;
use crate::engine::Priority;
use crate::events;
use crate::service::ApiResult;
use crate::service::AppService;
use crate::service::Inner;
use crate::service::StudyStart;
use crate::service::now_ms;

impl CaptureHost for Inner {
    fn record_capture_moves(&self, study_id: &str, from: NodeId, moves: &[Move]) -> Result<NodeId, String> {
        let result = self.with_study(study_id, |s| {
            let mut cur = from;
            let mut created = Vec::new();
            for mv in moves {
                let outcome = s.apply_move(cur, *mv, MoveSource::Capture, now_ms())?;
                if outcome.created {
                    created.push(outcome.node);
                }
                cur = outcome.node;
            }
            s.set_live(Some(cur), now_ms());
            Ok((cur, created, s.clone()))
        });
        match result {
            Ok((cur, created, study)) => {
                self.on_new_nodes(&study, &created, "capture", Priority::Background);
                if created.is_empty() {
                    self.emit_study_updated(&study, "capture", &[]);
                }
                Ok(cur)
            }
            Err(e) => Err(e.message),
        }
    }

    fn capture_state_changed(&self, state: &CaptureState) {
        if let Ok(mut s) = self.capture_state.lock() {
            *s = state.clone();
        }
        if let CaptureState::Stopped { message, .. } = state {
            self.notice(format!("屏幕连接已停止：{message}"));
        }
        events::emit(self.events.as_ref(), events::CAPTURE_STATE, state);
    }
}

fn parse_board(board: &str, side: xq_core::Color) -> ApiResult<Position> {
    let parsed = fen::parse_fen(board).map_err(|e| e.to_string())?;
    let pos = Position::new(parsed.board, side);
    let report = validate::validate_position(&pos);
    if !report.ok {
        let reasons: Vec<String> = report.issues.into_iter().map(|i| i.message).collect();
        return Err(format!("局面不成立：{}", reasons.join("；")).into());
    }
    Ok(pos)
}

fn bottom_side(orientation: xq_core::study::Orientation) -> xq_core::Color {
    match orientation {
        xq_core::study::Orientation::RedBottom => xq_core::Color::Red,
        xq_core::study::Orientation::BlackBottom => xq_core::Color::Black,
    }
}

impl AppService {
    pub fn list_windows(&self) -> ApiResult<Vec<WindowInfo>> { Ok(self.inner.capture_backend.list_windows()?) }

    pub fn capture_snapshot(&self, window_id: u32) -> ApiResult<Snapshot> { Ok(self.inner.capture_backend.snapshot(window_id)?) }

    pub fn capture_state(&self) -> CaptureState {
        self.inner.capture_state.lock().map(|s| s.clone()).unwrap_or(CaptureState::Idle)
    }

    pub fn start_capture(&self, req: StartCaptureRequest) -> ApiResult<CaptureState> {
        if let Some(old) = self.inner.capture.lock().ok().and_then(|mut c| c.take()) {
            old.stop();
        }
        let status = self.inner.capture_backend.status();
        if !status.available {
            return Err(format!("屏幕识别不可用：{}。仍可通过摆棋、FEN 或棋谱开始学习", status.detail).into());
        }
        let window = self
            .inner
            .capture_backend
            .list_windows()?
            .into_iter()
            .find(|w| w.id == req.window_id)
            .ok_or("所选窗口已不存在，请重新选择")?;
        let source = self.inner.capture_backend.open(req.window_id, req.rect)?;
        let host: Weak<dyn CaptureHost> = Arc::downgrade(&self.inner) as Weak<Inner>;
        let session = CaptureSession::start(source, window, self.inner.config_snapshot().capture, host)?;
        let state = session.state();
        if let Ok(mut c) = self.inner.capture.lock() {
            *c = Some(session);
        }
        self.inner.capture_state_changed(&state);
        Ok(state)
    }

    pub fn stop_capture(&self) -> CaptureState {
        let live = self.live_study();
        let session = self.inner.capture.lock().ok().and_then(|mut c| c.take());
        if let Some(session) = session {
            session.stop();
        }
        if let Some(study_id) = live {
            let _ = self.inner.with_study(&study_id, |s| {
                s.set_live(None, now_ms());
                Ok(())
            });
        }
        self.inner.capture_state_changed(&CaptureState::Idle);
        CaptureState::Idle
    }

    fn live_study(&self) -> Option<String> {
        match self.capture_state() {
            CaptureState::Tracking { study_id, .. } | CaptureState::AwaitingConfirmation { study_id, .. } => {
                Some(study_id)
            }
            _ => None,
        }
    }

    fn with_session<R>(&self, f: impl FnOnce(&CaptureSession) -> R) -> ApiResult<R> {
        let guard = self.inner.capture.lock().map_err(|_| "屏幕连接状态异常")?;
        let session = guard.as_ref().ok_or("屏幕连接没有运行")?;
        Ok(f(session))
    }

    /// 确认起始局面（可校正棋子并选择行棋方），开始记录。
    pub fn confirm_capture_start(&self, req: ConfirmStartRequest) -> ApiResult<StudyView> {
        // 先确认会话存在，再改动学习数据
        self.with_session(|_| ())?;
        let pos = parse_board(&req.board, req.side)?;
        let (view, node) = match req.attach {
            Some(target) => {
                let (node, study) = self.inner.with_study(&target.study_id, |s| {
                    let node = s.add_gap(target.node_id, &pos, "屏幕连接：从确认的画面局面继续", now_ms())?;
                    s.set_live(Some(node), now_ms());
                    Ok((node, s.clone()))
                })?;
                self.inner.emit_study_updated(&study, "capture", &[node]);
                (self.inner.study_view(&study)?, node)
            }
            None => {
                let name = req.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "屏幕对局".to_string());
                let view = self.inner.new_study(
                    name,
                    StudySource::Capture,
                    StudyStart { pos: pos.clone(), counters: None, history_complete: pos.is_startpos() },
                    Some(req.orientation),
                    Some(bottom_side(req.orientation)),
                    &[],
                )?;
                let study = self.inner.with_study(&view.study.id, |s| {
                    s.set_live(Some(ROOT), now_ms());
                    Ok(s.clone())
                })?;
                (self.inner.study_view(&study)?, ROOT)
            }
        };
        let state = self.with_session(|s| s.begin_tracking(&view.study.id, node, pos, req.orientation))?;
        self.inner.capture_state_changed(&state);
        Ok(view)
    }

    /// 处理无法解释的画面变化或新一局。
    pub fn resolve_capture(&self, req: ResolveCaptureRequest) -> ApiResult<CaptureState> {
        let (study_id, node_id, orientation) =
            self.with_session(|s| s.tracking_target())?.ok_or("屏幕连接尚未开始记录")?;
        let state = match req {
            ResolveCaptureRequest::Dismiss => self.with_session(|s| s.dismiss())?,
            ResolveCaptureRequest::Accept { board, side } => {
                let pos = parse_board(&board, side)?;
                let (gap, study) = self.inner.with_study(&study_id, |s| {
                    let gap = s.add_gap(node_id, &pos, "画面变化无法用合法走法解释，按确认的画面继续", now_ms())?;
                    s.set_live(Some(gap), now_ms());
                    Ok((gap, s.clone()))
                })?;
                self.inner.emit_study_updated(&study, "capture", &[gap]);
                self.with_session(|s| s.accept(&study_id, gap, pos))?
            }
            ResolveCaptureRequest::NewStudy { board, side } => {
                let pos = parse_board(&board, side)?;
                let view = self.inner.new_study(
                    "屏幕对局".into(),
                    StudySource::Capture,
                    StudyStart { pos: pos.clone(), counters: None, history_complete: pos.is_startpos() },
                    Some(orientation),
                    Some(bottom_side(orientation)),
                    &[],
                )?;
                let _ = self.inner.with_study(&study_id, |s| {
                    s.set_live(None, now_ms());
                    Ok(())
                });
                let study = self.inner.with_study(&view.study.id, |s| {
                    s.set_live(Some(ROOT), now_ms());
                    Ok(s.clone())
                })?;
                self.inner.emit_study_updated(&study, "capture", &[]);
                self.with_session(|s| s.accept(&study.id, ROOT, pos))?
            }
        };
        self.inner.capture_state_changed(&state);
        Ok(state)
    }

    /// 回到定位阶段，重新识别起始局面。
    pub fn relocate_capture(&self) -> ApiResult<CaptureState> {
        let state = self.with_session(|s| s.relocate())?;
        self.inner.capture_state_changed(&state);
        Ok(state)
    }
}
