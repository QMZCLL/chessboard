//! 走法规则：控制范围、合法走法、将军、将死与困毙判断，以及非法走法的具体原因。

use serde::Deserialize;
use serde::Serialize;

use crate::board::Board;
use crate::board::Position;
use crate::types::Color;
use crate::types::Move;
use crate::types::Piece;
use crate::types::PieceKind;
use crate::types::Square;

const ORTHO: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const DIAG: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
/// (落点偏移, 马腿偏移)
const HORSE: [((i8, i8), (i8, i8)); 8] = [
    ((1, 2), (0, 1)),
    ((-1, 2), (0, 1)),
    ((1, -2), (0, -1)),
    ((-1, -2), (0, -1)),
    ((2, 1), (1, 0)),
    ((2, -1), (1, 0)),
    ((-2, 1), (-1, 0)),
    ((-2, -1), (-1, 0)),
];

/// 棋子"控制"的交叉点：若该点有对方棋子则可以吃掉，不考虑走后是否送将。
/// 用于判断攻击与保护关系；炮的控制点是炮架之后直到下一个棋子（含）的所有点。
pub fn controls(board: &Board, from: Square) -> Vec<Square> {
    let Some(piece) = board.get(from) else { return Vec::new() };
    let mut out = Vec::new();
    match piece.kind {
        PieceKind::King => {
            for (df, dr) in ORTHO {
                if let Some(t) = from.offset(df, dr)
                    && piece.color.in_palace(t)
                {
                    out.push(t);
                }
            }
            // 将帅照面：同一纵线中间无子时视为可以"吃"到对方将帅
            let mut cur = from;
            while let Some(next) = cur.offset(0, piece.color.forward()) {
                if let Some(p) = board.get(next) {
                    if p.kind == PieceKind::King && p.color != piece.color {
                        out.push(next);
                    }
                    break;
                }
                cur = next;
            }
        }
        PieceKind::Advisor => {
            for (df, dr) in DIAG {
                if let Some(t) = from.offset(df, dr)
                    && piece.color.in_palace(t)
                {
                    out.push(t);
                }
            }
        }
        PieceKind::Elephant => {
            for (df, dr) in DIAG {
                if let (Some(eye), Some(t)) = (from.offset(df, dr), from.offset(2 * df, 2 * dr))
                    && board.get(eye).is_none()
                    && piece.color.own_half(t.rank())
                {
                    out.push(t);
                }
            }
        }
        PieceKind::Horse => {
            for ((tf, tr), (lf, lr)) in HORSE {
                if let (Some(leg), Some(t)) = (from.offset(lf, lr), from.offset(tf, tr))
                    && board.get(leg).is_none()
                {
                    out.push(t);
                }
            }
        }
        PieceKind::Rook => {
            for (df, dr) in ORTHO {
                let mut cur = from;
                while let Some(next) = cur.offset(df, dr) {
                    out.push(next);
                    if board.get(next).is_some() {
                        break;
                    }
                    cur = next;
                }
            }
        }
        PieceKind::Cannon => {
            for (df, dr) in ORTHO {
                let mut cur = from;
                let mut screened = false;
                while let Some(next) = cur.offset(df, dr) {
                    let occupied = board.get(next).is_some();
                    if screened {
                        out.push(next);
                        if occupied {
                            break;
                        }
                    } else if occupied {
                        screened = true;
                    }
                    cur = next;
                }
            }
        }
        PieceKind::Pawn => {
            if let Some(t) = from.offset(0, piece.color.forward()) {
                out.push(t);
            }
            if !piece.color.own_half(from.rank()) {
                for df in [-1, 1] {
                    if let Some(t) = from.offset(df, 0) {
                        out.push(t);
                    }
                }
            }
        }
    }
    out
}

/// 伪合法落点：符合棋子走法且不吃己方棋子，但不检查走后是否送将。
pub fn pseudo_targets(board: &Board, from: Square) -> Vec<Square> {
    let Some(piece) = board.get(from) else { return Vec::new() };
    let not_own = |t: &Square| board.get(*t).is_none_or(|p| p.color != piece.color);
    match piece.kind {
        PieceKind::Cannon => {
            let mut out = Vec::new();
            for (df, dr) in ORTHO {
                let mut cur = from;
                let mut screened = false;
                while let Some(next) = cur.offset(df, dr) {
                    match (screened, board.get(next)) {
                        (false, None) => out.push(next),
                        (false, Some(_)) => screened = true,
                        (true, None) => {}
                        (true, Some(p)) => {
                            if p.color != piece.color {
                                out.push(next);
                            }
                            break;
                        }
                    }
                    cur = next;
                }
            }
            out
        }
        PieceKind::King => ORTHO
            .iter()
            .filter_map(|(df, dr)| from.offset(*df, *dr))
            .filter(|t| piece.color.in_palace(*t))
            .filter(not_own)
            .collect(),
        _ => controls(board, from).into_iter().filter(not_own).collect(),
    }
}

/// `by` 一方控制 `sq` 的所有棋子位置。
pub fn attackers(board: &Board, sq: Square, by: Color) -> Vec<Square> {
    board.pieces().filter(|(from, p)| p.color == by && controls(board, *from).contains(&sq)).map(|(from, _)| from).collect()
}

pub fn is_attacked(board: &Board, sq: Square, by: Color) -> bool {
    board.pieces().any(|(from, p)| p.color == by && controls(board, from).contains(&sq))
}

pub fn kings_facing(board: &Board) -> bool {
    let (Some(red), Some(black)) = (board.find_king(Color::Red), board.find_king(Color::Black)) else {
        return false;
    };
    if red.file() != black.file() {
        return false;
    }
    let (low, high) = (red.rank().min(black.rank()), red.rank().max(black.rank()));
    ((low + 1)..high).all(|rank| board.get(Square::new(red.file(), rank).expect("同一纵线")).is_none())
}

/// `color` 的将帅是否正受攻击（含将帅照面）。缺少将帅时返回 false，由局面校验报告。
pub fn in_check(board: &Board, color: Color) -> bool {
    board.find_king(color).is_some_and(|king| is_attacked(board, king, color.opposite()))
}

pub fn checkers(board: &Board, color: Color) -> Vec<Square> {
    board.find_king(color).map(|king| attackers(board, king, color.opposite())).unwrap_or_default()
}

pub fn is_legal(pos: &Position, mv: Move) -> bool {
    let Some(piece) = pos.board.get(mv.from) else { return false };
    piece.color == pos.side
        && pseudo_targets(&pos.board, mv.from).contains(&mv.to)
        && !in_check(&pos.board.applied(mv), pos.side)
}

pub fn legal_moves_from(pos: &Position, from: Square) -> Vec<Move> {
    match pos.board.get(from) {
        Some(p) if p.color == pos.side => pseudo_targets(&pos.board, from)
            .into_iter()
            .map(|to| Move::new(from, to))
            .filter(|mv| !in_check(&pos.board.applied(*mv), pos.side))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn legal_moves(pos: &Position) -> Vec<Move> {
    let own: Vec<Square> = pos.board.pieces().filter(|(_, p)| p.color == pos.side).map(|(sq, _)| sq).collect();
    own.into_iter().flat_map(|sq| legal_moves_from(pos, sq)).collect()
}

/// 对局状态。象棋中轮到一方而无合法着法时该方判负：被将军为将死，未被将军为困毙。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    Ongoing,
    Checkmate,
    Stalemate,
}

pub fn game_status(pos: &Position) -> GameStatus {
    if !legal_moves(pos).is_empty() {
        GameStatus::Ongoing
    } else if in_check(&pos.board, pos.side) {
        GameStatus::Checkmate
    } else {
        GameStatus::Stalemate
    }
}

/// 非法走法的具体原因，供界面直接展示。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IllegalMove {
    pub code: &'static str,
    pub message: String,
    pub squares: Vec<Square>,
}

fn illegal(code: &'static str, message: impl Into<String>, squares: Vec<Square>) -> IllegalMove {
    IllegalMove { code, message: message.into(), squares }
}

fn describe(piece: Piece, sq: Square) -> String { format!("{}的{}", sq, piece.name()) }

/// 合法时返回 None，否则说明违反了哪条规则。
pub fn illegal_reason(pos: &Position, mv: Move) -> Option<IllegalMove> {
    let board = &pos.board;
    let Some(piece) = board.get(mv.from) else {
        return Some(illegal("no_piece", format!("{} 没有棋子", mv.from), vec![mv.from]));
    };
    if piece.color != pos.side {
        return Some(illegal(
            "not_your_turn",
            format!("现在轮到{}走，不能移动{}的{}", pos.side.label(), piece.color.label(), piece.name()),
            vec![mv.from],
        ));
    }
    if mv.from == mv.to {
        return Some(illegal("same_square", "起点和落点相同", vec![mv.from]));
    }
    if let Some(target) = board.get(mv.to)
        && target.color == piece.color
    {
        return Some(illegal("own_piece", format!("不能吃己方的{}", target.name()), vec![mv.to]));
    }
    if !pseudo_targets(board, mv.from).contains(&mv.to) {
        return Some(movement_reason(board, piece, mv));
    }
    let after = board.applied(mv);
    if in_check(&after, piece.color) {
        let own_king = after.find_king(piece.color);
        if kings_facing(&after) {
            let squares = [own_king, after.find_king(piece.color.opposite())].into_iter().flatten().collect();
            return Some(illegal("kings_facing", "走后将帅在同一纵线上直接照面，不合法", squares));
        }
        let mut squares: Vec<Square> = checkers(&after, piece.color);
        squares.extend(own_king);
        if in_check(board, piece.color) {
            return Some(illegal("still_in_check", format!("{}正被将军，这步没有解除将军", pos.side.label()), squares));
        }
        let king_name = Piece::new(piece.color, PieceKind::King).name();
        let attackers: Vec<String> =
            checkers(&after, piece.color).iter().filter_map(|sq| after.get(*sq).map(|p| describe(p, *sq))).collect();
        return Some(illegal(
            "exposes_king",
            format!("走后己方{}会被{}直接攻击（送将）", king_name, attackers.join("、")),
            squares,
        ));
    }
    None
}

fn between(from: Square, to: Square) -> Vec<Square> {
    let df = (to.file() as i8 - from.file() as i8).signum();
    let dr = (to.rank() as i8 - from.rank() as i8).signum();
    let mut out = Vec::new();
    let mut cur = from;
    while let Some(next) = cur.offset(df, dr) {
        if next == to {
            break;
        }
        out.push(next);
        cur = next;
    }
    out
}

fn movement_reason(board: &Board, piece: Piece, mv: Move) -> IllegalMove {
    let df = mv.to.file() as i8 - mv.from.file() as i8;
    let dr = mv.to.rank() as i8 - mv.from.rank() as i8;
    let straight = df == 0 || dr == 0;
    let name = piece.name();
    let blockers = |line: &[Square]| -> Vec<Square> { line.iter().copied().filter(|sq| board.get(*sq).is_some()).collect() };
    match piece.kind {
        PieceKind::King => {
            if !piece.color.in_palace(mv.to) {
                illegal("king_palace", format!("{name}不能离开九宫"), vec![mv.to])
            } else {
                illegal("king_step", format!("{name}每次只能横向或纵向走一格"), vec![mv.from, mv.to])
            }
        }
        PieceKind::Advisor => {
            if !piece.color.in_palace(mv.to) {
                illegal("advisor_palace", format!("{name}不能离开九宫"), vec![mv.to])
            } else {
                illegal("advisor_step", format!("{name}只能在九宫内斜走一格"), vec![mv.from, mv.to])
            }
        }
        PieceKind::Elephant => {
            if df.abs() == 2 && dr.abs() == 2 {
                if !piece.color.own_half(mv.to.rank()) {
                    return illegal("elephant_river", format!("{name}不能过河"), vec![mv.to]);
                }
                let eye = mv.from.offset(df / 2, dr / 2).expect("田字中心在棋盘内");
                if let Some(blocker) = board.get(eye) {
                    return illegal("elephant_eye", format!("塞象眼：{}挡在田字中心", describe(blocker, eye)), vec![eye]);
                }
            }
            illegal("elephant_shape", format!("{name}只能走“田”字"), vec![mv.from, mv.to])
        }
        PieceKind::Horse => {
            if (df.abs(), dr.abs()) == (1, 2) || (df.abs(), dr.abs()) == (2, 1) {
                let leg = if dr.abs() == 2 {
                    mv.from.offset(0, dr.signum())
                } else {
                    mv.from.offset(df.signum(), 0)
                }
                .expect("马腿在棋盘内");
                if let Some(blocker) = board.get(leg) {
                    return illegal("horse_leg", format!("蹩马腿：{}挡住了马腿", describe(blocker, leg)), vec![leg]);
                }
            }
            illegal("horse_shape", "马只能走“日”字", vec![mv.from, mv.to])
        }
        PieceKind::Rook => {
            if !straight {
                return illegal("rook_line", "车只能沿横线或纵线走", vec![mv.from, mv.to]);
            }
            let blocked = blockers(&between(mv.from, mv.to));
            let first = blocked.first().copied();
            let message = match first.and_then(|sq| board.get(sq).map(|p| describe(p, sq))) {
                Some(desc) => format!("车的路线被{desc}挡住"),
                None => "车的路线被挡住".to_string(),
            };
            illegal("rook_blocked", message, blocked)
        }
        PieceKind::Cannon => {
            if !straight {
                return illegal("cannon_line", "炮只能沿横线或纵线走", vec![mv.from, mv.to]);
            }
            let screens = blockers(&between(mv.from, mv.to));
            if board.get(mv.to).is_none() {
                illegal("cannon_blocked", "炮不吃子时，路线上不能有棋子", screens)
            } else if screens.is_empty() {
                illegal("cannon_no_screen", "炮吃子必须隔一个棋子（炮架），当前中间没有棋子", vec![mv.from, mv.to])
            } else {
                illegal("cannon_many_screens", format!("炮吃子只能隔一个棋子，当前中间有 {} 个", screens.len()), screens)
            }
        }
        PieceKind::Pawn => {
            if dr * piece.color.forward() < 0 {
                illegal("pawn_backward", format!("{name}不能后退"), vec![mv.from, mv.to])
            } else if dr == 0 && piece.color.own_half(mv.from.rank()) {
                illegal("pawn_sideways", format!("{name}过河之前只能向前走"), vec![mv.from, mv.to])
            } else {
                illegal("pawn_step", format!("{name}每次只能走一格"), vec![mv.from, mv.to])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_position;

    fn perft(pos: &Position, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        legal_moves(pos).into_iter().map(|mv| perft(&pos.after(mv), depth - 1)).sum()
    }

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    #[test]
    fn perft_startpos() {
        let pos = Position::startpos();
        assert_eq!(perft(&pos, 1), 44);
        assert_eq!(perft(&pos, 2), 1_920);
        assert_eq!(perft(&pos, 3), 79_666);
    }

    #[test]
    fn flying_general_blocks_king_move() {
        let pos = parse_position("3k5/9/9/9/9/9/9/9/9/4K4 w").unwrap();
        let err = illegal_reason(&pos, mv("e0d0")).unwrap();
        assert_eq!(err.code, "kings_facing");
        assert!(is_legal(&pos, mv("e0e1")));
    }

    #[test]
    fn terminal_positions() {
        // 黑将无子可动且未被将军：困毙
        let stalemate = parse_position("3k5/4R4/4R4/9/9/9/9/9/9/4K4 b").unwrap();
        assert_eq!(game_status(&stalemate), GameStatus::Stalemate);
        // 双车错杀：被将军且无法应将
        let mate = parse_position("3k5/3R5/3R5/9/9/9/9/9/9/4K4 b").unwrap();
        assert_eq!(game_status(&mate), GameStatus::Checkmate);
        assert_eq!(game_status(&Position::startpos()), GameStatus::Ongoing);
    }

    #[test]
    fn illegal_reasons_are_specific() {
        let pos = Position::startpos();
        assert!(illegal_reason(&pos, mv("h0g2")).is_none());
        // h0→f1 是日字，但马腿 g0 有相
        assert_eq!(illegal_reason(&pos, mv("h0f1")).unwrap().code, "horse_leg");
        assert_eq!(illegal_reason(&pos, mv("h0h1")).unwrap().code, "horse_shape");
        assert_eq!(illegal_reason(&pos, mv("c0e2")), None);
        assert_eq!(illegal_reason(&pos, mv("b2b7")).unwrap().code, "cannon_no_screen");
        assert_eq!(illegal_reason(&pos, mv("b2b8")).unwrap().code, "cannon_blocked");
        assert_eq!(illegal_reason(&pos, mv("b2b9")), None);
        assert_eq!(illegal_reason(&pos, mv("a0a5")).unwrap().code, "rook_blocked");
        assert_eq!(illegal_reason(&pos, mv("e3d3")).unwrap().code, "pawn_sideways");
        assert_eq!(illegal_reason(&pos, mv("h9g7")).unwrap().code, "not_your_turn");
        assert_eq!(illegal_reason(&pos, mv("e0e1")), None);
        assert_eq!(illegal_reason(&pos, mv("e0e2")).unwrap().code, "king_step");
        // 蹩马腿：b0 马跳 a2 时马腿 b1 为空，跳 d1 时马腿 c0 有相
        assert_eq!(illegal_reason(&pos, mv("b0d1")).unwrap().code, "horse_leg");
        // 炮隔两个子不能吃
        let pos = parse_position("r3k4/9/9/9/p8/P8/9/9/9/C2K5 w").unwrap();
        assert_eq!(illegal_reason(&pos, mv("a0a9")).unwrap().code, "cannon_many_screens");
    }

    #[test]
    fn exposing_king_is_rejected() {
        // 红车 e5 挡在将帅之间，离开纵线会导致照面
        let pos = parse_position("4k4/9/9/9/4R4/9/9/9/9/4K4 w").unwrap();
        assert_eq!(illegal_reason(&pos, mv("e5d5")).unwrap().code, "kings_facing");
        // 被将军时走其他棋子
        let pos = parse_position("4k4/9/9/9/9/9/9/9/3r5/3K1A3 w").unwrap();
        assert!(in_check(&pos.board, Color::Red));
        assert_eq!(illegal_reason(&pos, mv("f0e1")).unwrap().code, "still_in_check");
        assert_eq!(illegal_reason(&pos, mv("d0d1")), None);
    }
}
