//! 屏幕识别结果的几何换算与走子确认。
//!
//! 识别结果只是待确认的观察数据：连续多帧一致后，再用合法走法解释它与上一个
//! 已确认局面的差异；无法解释时交给用户确认或校正，而不是直接改写棋局。

use serde::Deserialize;
use serde::Serialize;

use crate::board::Board;
use crate::board::Position;
use crate::notation;
use crate::rules;
use crate::study::Orientation;
use crate::types::Color;
use crate::types::Move;
use crate::types::Piece;
use crate::types::PieceKind;
use crate::types::Square;
use crate::validate;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DetectedClass {
    Board,
    Piece(Piece),
}

/// 模型输出坐标系下的一个检测框。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub class: DetectedClass,
    pub confidence: f32,
}

/// 由整窗识别结果计算棋盘裁剪区（原图坐标）。
///
/// 与模型训练时的约定一致：棋盘框四周各外扩半格，使裁剪图按 9×10 等分后每格中心落在交叉点上。
pub fn board_crop(dets: &[Detection], model_w: f32, model_h: f32, image_w: u32, image_h: u32) -> Option<Rect> {
    let board = dets.iter().filter(|d| d.class == DetectedClass::Board).max_by(|a, b| a.confidence.total_cmp(&b.confidence))?;
    let (iw, ih) = (image_w as f32, image_h as f32);
    let sx = iw / model_w;
    let sy = ih / model_h;
    let bx0 = (board.x0 * sx).max(0.0);
    let by0 = (board.y0 * sy).max(0.0);
    let bx1 = (board.x1 * sx).min(iw);
    let by1 = (board.y1 * sy).min(ih);
    if bx1 <= bx0 || by1 <= by0 {
        return None;
    }
    let half_x = (bx1 - bx0) / 9.0 / 2.0;
    let half_y = (by1 - by0) / 10.0 / 2.0;
    let x = (bx0 - half_x).max(0.0).floor();
    let y = (by0 - half_y).max(0.0).floor();
    let w = ((bx1 + half_x).min(iw) - x).floor();
    let h = ((by1 + half_y).min(ih) - y).floor();
    (w >= 9.0 && h >= 10.0).then_some(Rect { x, y, w, h })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct CellReading {
    pub piece: Option<Piece>,
    pub confidence: f32,
    /// 同一格有多个不同的检测结果。
    pub conflict: bool,
}

/// 按画面方向（行从上到下、列从左到右）记录的识别结果。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Observation {
    pub cells: [[CellReading; 9]; 10],
    pub board_found: bool,
}

/// 把裁剪图上的检测框落到 9×10 格中。`width`/`height` 为模型输入尺寸。
pub fn observe(dets: &[Detection], width: f32, height: f32) -> Observation {
    let mut cells = [[CellReading::default(); 9]; 10];
    let cell_w = width / 9.0;
    let cell_h = height / 10.0;
    for det in dets {
        let DetectedClass::Piece(piece) = det.class else { continue };
        let cx = (det.x0 + det.x1) / 2.0;
        let cy = (det.y0 + det.y1) / 2.0;
        if cx < 0.0 || cy < 0.0 {
            continue;
        }
        let (col, row) = ((cx / cell_w).floor() as usize, (cy / cell_h).floor() as usize);
        if col > 8 || row > 9 {
            continue;
        }
        let cell = &mut cells[row][col];
        match cell.piece {
            None => *cell = CellReading { piece: Some(piece), confidence: det.confidence, conflict: false },
            Some(existing) => {
                if existing != piece {
                    cell.conflict = true;
                }
                if det.confidence > cell.confidence {
                    cell.piece = Some(piece);
                    cell.confidence = det.confidence;
                }
            }
        }
    }
    Observation { cells, board_found: dets.iter().any(|d| d.class == DetectedClass::Board) }
}

/// 画面行列与标准坐标的换算。
pub fn screen_to_square(row: usize, col: usize, orientation: Orientation) -> Option<Square> {
    let sq = Square::new(col as u8, 9u8.checked_sub(row as u8)?)?;
    Some(match orientation {
        Orientation::RedBottom => sq,
        Orientation::BlackBottom => sq.rotated(),
    })
}

/// 根据将帅所在半区推断棋盘方向。
pub fn infer_orientation(obs: &Observation) -> Option<Orientation> {
    let mut red_row = None;
    let mut black_row = None;
    for (row, line) in obs.cells.iter().enumerate() {
        for cell in line {
            match cell.piece {
                Some(Piece { color: Color::Red, kind: PieceKind::King }) => red_row = Some(row),
                Some(Piece { color: Color::Black, kind: PieceKind::King }) => black_row = Some(row),
                _ => {}
            }
        }
    }
    match (red_row, black_row) {
        (Some(r), _) if r >= 5 => Some(Orientation::RedBottom),
        (_, Some(b)) if b >= 5 => Some(Orientation::BlackBottom),
        (Some(_), _) => Some(Orientation::BlackBottom),
        (_, Some(_)) => Some(Orientation::RedBottom),
        _ => None,
    }
}

/// 识别结果换算为标准方向的棋盘。
pub fn to_board(obs: &Observation, orientation: Orientation) -> Board {
    let mut board = Board::empty();
    for (row, line) in obs.cells.iter().enumerate() {
        for (col, cell) in line.iter().enumerate() {
            if let (Some(piece), Some(sq)) = (cell.piece, screen_to_square(row, col, orientation)) {
                board.set(sq, Some(piece));
            }
        }
    }
    board
}

/// 置信度偏低或有冲突、需要用户留意的格子（标准坐标）。
pub fn uncertain_squares(obs: &Observation, orientation: Orientation, threshold: f32) -> Vec<Square> {
    let mut out = Vec::new();
    for (row, line) in obs.cells.iter().enumerate() {
        for (col, cell) in line.iter().enumerate() {
            if cell.conflict || (cell.piece.is_some() && cell.confidence < threshold) {
                out.extend(screen_to_square(row, col, orientation));
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub struct TrackerConfig {
    /// 画面连续一致多少帧才视为稳定（过滤走子动画）。
    pub stable_frames: u32,
    /// 无法解释的稳定画面再持续多少帧才请求用户确认。
    pub unexplained_frames: u32,
}

impl Default for TrackerConfig {
    fn default() -> Self { Self { stable_frames: 3, unexplained_frames: 5 } }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrackEvent {
    Idle,
    /// 确认的一步或连续两步合法走法（两步时说明漏掉了中间一帧）。
    Moves(Vec<Move>),
    /// 画面回到标准开局，等待用户确认是否开始新的一局；确认前保持原来的局面与棋谱。
    NewGame { board: Board },
    /// 画面稳定但无法用合法走法解释，需要确认或校正；确认前不改写棋局。
    NeedsConfirmation { board: Board, reason: String },
    /// 等待确认期间画面又发生变化：只更新待确认的画面，不记录走子。
    PendingUpdate { board: Board },
    /// 等待确认期间画面回到了已确认局面，确认请求不再需要。
    Resolved,
}

pub struct CaptureTracker {
    cfg: TrackerConfig,
    confirmed: Position,
    candidate: Option<Board>,
    count: u32,
    /// 正在等待用户确认的画面。等待期间不提交任何走子。
    pending: Option<Board>,
    /// 用户选择“暂不处理”的画面：保持原局面，不再为同一画面重复提示。
    dismissed: Option<Board>,
}

impl CaptureTracker {
    pub fn new(confirmed: Position, cfg: TrackerConfig) -> Self {
        Self { cfg, confirmed, candidate: None, count: 0, pending: None, dismissed: None }
    }

    pub fn confirmed(&self) -> &Position { &self.confirmed }

    pub fn is_waiting(&self) -> bool { self.pending.is_some() }

    /// 用户暂不处理当前的确认请求：保持原来的已确认局面；同一画面不再提示，画面变化后照常处理。
    pub fn resume(&mut self) {
        self.dismissed = self.pending.take();
        self.candidate = None;
        self.count = 0;
    }

    /// 用户确认或校正后，从新的已确认局面继续。
    pub fn reset(&mut self, pos: Position) {
        self.confirmed = pos;
        self.candidate = None;
        self.count = 0;
        self.pending = None;
        self.dismissed = None;
    }

    pub fn feed(&mut self, board: Board) -> TrackEvent {
        if self.candidate.as_ref() == Some(&board) {
            self.count = self.count.saturating_add(1);
        } else {
            self.candidate = Some(board.clone());
            self.count = 1;
        }
        if self.count < self.cfg.stable_frames {
            return TrackEvent::Idle;
        }
        if let Some(pending) = &self.pending {
            if board == self.confirmed.board {
                self.pending = None;
                return TrackEvent::Resolved;
            }
            if *pending != board {
                self.pending = Some(board.clone());
                return TrackEvent::PendingUpdate { board };
            }
            return TrackEvent::Idle;
        }
        if board == self.confirmed.board {
            self.dismissed = None;
            return TrackEvent::Idle;
        }
        if self.dismissed.as_ref() == Some(&board) {
            return TrackEvent::Idle;
        }
        // 画面恰好回到开局几乎总是新的一局；先于走法解释判断，避免把“新开局”当成退回的着法
        if board == Board::startpos() {
            self.pending = Some(board.clone());
            return TrackEvent::NewGame { board };
        }
        if let Some(moves) = explain_change(&self.confirmed, &board) {
            let mut pos = self.confirmed.clone();
            for mv in &moves {
                pos = pos.after(*mv);
            }
            self.reset(pos);
            return TrackEvent::Moves(moves);
        }
        if self.count >= self.cfg.stable_frames + self.cfg.unexplained_frames {
            self.pending = Some(board.clone());
            let reason = describe_unexplained(&self.confirmed, &board);
            return TrackEvent::NeedsConfirmation { board, reason };
        }
        TrackEvent::Idle
    }
}

fn differing_squares(a: &Board, b: &Board) -> usize { Square::all().filter(|sq| a.get(*sq) != b.get(*sq)).count() }

/// 用一步或两步合法走法解释从 `pos` 到 `target` 的变化。
pub fn explain_change(pos: &Position, target: &Board) -> Option<Vec<Move>> {
    let diff = differing_squares(&pos.board, target);
    if diff == 0 || diff > 4 {
        return None;
    }
    let legal = rules::legal_moves(pos);
    if let Some(mv) = legal.iter().find(|mv| pos.board.applied(**mv) == *target) {
        return Some(vec![*mv]);
    }
    let mut found = None;
    for first in &legal {
        let next = pos.after(*first);
        for second in rules::legal_moves(&next) {
            if next.board.applied(second) == *target {
                if found.is_some() {
                    // 两步解释不唯一时不猜测
                    return None;
                }
                found = Some(vec![*first, second]);
            }
        }
    }
    found
}

fn describe_unexplained(pos: &Position, target: &Board) -> String {
    let issues: Vec<String> = validate::validate_board(target).into_iter().map(|i| i.message).collect();
    if !issues.is_empty() {
        return format!("识别结果不是有效局面：{}", issues.join("；"));
    }
    let other = Position::new(pos.board.clone(), pos.side.opposite());
    if let Some(mv) = rules::legal_moves(&other).into_iter().find(|mv| other.board.applied(*mv) == *target) {
        let text = notation::move_to_chinese(&other, mv).unwrap_or_else(|_| mv.iccs());
        return format!(
            "画面变化像是{}走了“{}”，与当前记录的行棋方（{}）不一致，请确认行棋方",
            other.side.label(),
            text,
            pos.side.label()
        );
    }
    "无法用一到两步合法走法解释画面变化，可能漏帧、遮挡或识别错误".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_position;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    fn feed_n(t: &mut CaptureTracker, b: &Board, n: u32) -> Vec<TrackEvent> { (0..n).map(|_| t.feed(b.clone())).collect() }

    #[test]
    fn orientation_and_board_mapping() {
        let start = Board::startpos();
        // 构造黑方在下的画面：标准棋盘旋转 180°
        let mut obs = Observation { cells: [[CellReading::default(); 9]; 10], board_found: true };
        for (sq, p) in start.pieces() {
            let screen = sq.rotated();
            obs.cells[9 - screen.rank() as usize][screen.file() as usize] =
                CellReading { piece: Some(p), confidence: 0.99, conflict: false };
        }
        assert_eq!(infer_orientation(&obs), Some(Orientation::BlackBottom));
        assert_eq!(to_board(&obs, Orientation::BlackBottom), start);
    }

    #[test]
    fn observe_resolves_duplicates() {
        let red_rook = Piece::new(Color::Red, PieceKind::Rook);
        let red_horse = Piece::new(Color::Red, PieceKind::Horse);
        let dets = [
            Detection { x0: 0.0, y0: 0.0, x1: 600.0, y1: 600.0, class: DetectedClass::Board, confidence: 0.9 },
            Detection { x0: 5.0, y0: 580.0, x1: 60.0, y1: 635.0, class: DetectedClass::Piece(red_rook), confidence: 0.8 },
            Detection { x0: 6.0, y0: 581.0, x1: 61.0, y1: 636.0, class: DetectedClass::Piece(red_horse), confidence: 0.9 },
        ];
        let obs = observe(&dets, 640.0, 640.0);
        let cell = obs.cells[9][0];
        assert_eq!(cell.piece, Some(red_horse));
        assert!(cell.conflict);
        let squares = uncertain_squares(&obs, Orientation::RedBottom, 0.85);
        assert_eq!(squares, vec![Square::parse("a0").unwrap()]);
    }

    #[test]
    fn crop_expands_half_cell() {
        let dets = [Detection { x0: 100.0, y0: 100.0, x1: 280.0, y1: 300.0, class: DetectedClass::Board, confidence: 0.9 }];
        let r = board_crop(&dets, 640.0, 640.0, 640, 640).unwrap();
        assert_eq!(r, Rect { x: 90.0, y: 90.0, w: 200.0, h: 220.0 });
    }

    #[test]
    fn tracker_confirms_stable_moves_only() {
        let mut t = CaptureTracker::new(Position::startpos(), TrackerConfig { stable_frames: 3, unexplained_frames: 2 });
        let after = Board::startpos().applied(mv("h2e2"));
        // 动画中间帧：只出现一次的画面不会被确认
        let mut mid = Board::startpos();
        mid.set(Square::parse("h2").unwrap(), None);
        assert_eq!(t.feed(mid), TrackEvent::Idle);
        let events = feed_n(&mut t, &after, 3);
        assert_eq!(events[2], TrackEvent::Moves(vec![mv("h2e2")]));
        assert_eq!(t.confirmed().side, Color::Black);
    }

    #[test]
    fn tracker_recovers_missed_frame_and_flags_gaps() {
        let mut t = CaptureTracker::new(Position::startpos(), TrackerConfig { stable_frames: 2, unexplained_frames: 2 });
        let two = Board::startpos().applied(mv("h2e2")).applied(mv("h9g7"));
        let events = feed_n(&mut t, &two, 2);
        assert_eq!(events[1], TrackEvent::Moves(vec![mv("h2e2"), mv("h9g7")]));

        // 多子同时变化：无法解释，稳定一段时间后只提示一次
        let weird = parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 w").unwrap().board;
        let events = feed_n(&mut t, &weird, 6);
        let asks: Vec<&TrackEvent> =
            events.iter().filter(|e| matches!(e, TrackEvent::NeedsConfirmation { .. })).collect();
        assert_eq!(asks.len(), 1);
        assert!(t.is_waiting());
    }

    #[test]
    fn new_game_waits_for_confirmation_without_committing() {
        let mut t = CaptureTracker::new(Position::startpos(), TrackerConfig { stable_frames: 1, unexplained_frames: 0 });
        // 先在一盘棋里走两步
        let played = Board::startpos().applied(mv("h2e2"));
        assert_eq!(t.feed(played.clone()), TrackEvent::Moves(vec![mv("h2e2")]));
        let played = played.applied(mv("h9g7"));
        assert_eq!(t.feed(played.clone()), TrackEvent::Moves(vec![mv("h9g7")]));
        let confirmed = t.confirmed().clone();
        // 外部棋盘回到开局：提示新一局，但保持原局面
        assert!(matches!(t.feed(Board::startpos()), TrackEvent::NewGame { .. }));
        assert_eq!(t.confirmed(), &confirmed);
        // 用户尚未确认，新一局已经走出第一步：只更新待确认画面，不能接到旧棋谱后面
        let first = Board::startpos().applied(mv("b2e2"));
        assert_eq!(t.feed(first.clone()), TrackEvent::PendingUpdate { board: first.clone() });
        assert_eq!(t.feed(first), TrackEvent::Idle);
        assert_eq!(t.confirmed(), &confirmed);
        // 画面回到原局面，确认请求自动取消
        assert_eq!(t.feed(played), TrackEvent::Resolved);
        assert!(!t.is_waiting());
    }

    #[test]
    fn dismissed_board_is_not_asked_again() {
        let mut t = CaptureTracker::new(Position::startpos(), TrackerConfig { stable_frames: 1, unexplained_frames: 1 });
        let weird = parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 w").unwrap().board;
        assert_eq!(t.feed(weird.clone()), TrackEvent::Idle);
        assert!(matches!(t.feed(weird.clone()), TrackEvent::NeedsConfirmation { .. }));
        t.resume();
        for _ in 0..5 {
            assert_eq!(t.feed(weird.clone()), TrackEvent::Idle);
        }
        assert_eq!(t.confirmed(), &Position::startpos(), "暂不处理时保持原局面");
        // 画面恢复后仍能正常记录走子
        assert_eq!(t.feed(Board::startpos().applied(mv("h2e2"))), TrackEvent::Moves(vec![mv("h2e2")]));
    }

    #[test]
    fn tracker_reports_side_mismatch() {
        // 记录为黑走，但画面上是红方又走了一步
        let pos = Position::startpos().after(mv("h2e2"));
        let mut t = CaptureTracker::new(pos.clone(), TrackerConfig { stable_frames: 1, unexplained_frames: 0 });
        let target = pos.board.applied(mv("h0g2"));
        match t.feed(target) {
            TrackEvent::NeedsConfirmation { reason, .. } => assert!(reason.contains("行棋方"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }
}
