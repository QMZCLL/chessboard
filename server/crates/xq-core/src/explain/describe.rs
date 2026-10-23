//! 讲解文字里的棋子称呼。

use crate::board::Board;
use crate::notation::file_number;
use crate::types::Color;
use crate::types::Piece;
use crate::types::PieceKind;
use crate::types::Square;

/// 车、马、炮在初始位置时的左右称呼（从该方视角）。
fn home_side(piece: Piece, sq: Square) -> Option<&'static str> {
    let home = matches!(piece.kind, PieceKind::Rook | PieceKind::Horse) && sq.rank() == piece.color.home_rank();
    let cannon_home = piece.kind == PieceKind::Cannon
        && sq.rank() == if piece.color == Color::Red { 2 } else { 7 }
        && (sq.file() == 1 || sq.file() == 7);
    if !(home || cannon_home) || sq.file() == 4 {
        return None;
    }
    let right = match piece.color {
        Color::Red => sq.file() > 4,
        Color::Black => sq.file() < 4,
    };
    Some(if right { "右" } else { "左" })
}

/// 棋子称呼，如“中兵”“三路兵”“7路卒”“右马”“中炮”“八路车”。
pub fn piece_label(piece: Piece, sq: Square) -> String {
    let name = piece.name();
    match piece.kind {
        PieceKind::King | PieceKind::Advisor | PieceKind::Elephant => name.to_string(),
        _ => {
            if let Some(side) = home_side(piece, sq) {
                return format!("{side}{name}");
            }
            if sq.file() == 4 && matches!(piece.kind, PieceKind::Pawn | PieceKind::Cannon | PieceKind::Horse) {
                return format!("中{name}");
            }
            format!("{}路{name}", file_number(piece.color, sq.file()))
        }
    }
}

pub fn label_at(board: &Board, sq: Square) -> String {
    board.get(sq).map(|p| piece_label(p, sq)).unwrap_or_else(|| sq.iccs())
}

/// 带归属的称呼，如“对方中卒”“己方右马”。
pub fn owned_label(board: &Board, sq: Square, viewer: Color) -> String {
    match board.get(sq) {
        Some(p) if p.color == viewer => format!("己方{}", piece_label(p, sq)),
        Some(p) => format!("对方{}", piece_label(p, sq)),
        None => sq.iccs(),
    }
}

pub fn join_labels(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        _ => format!("{}和{}", items[..items.len() - 1].join("、"), items[items.len() - 1]),
    }
}

/// 粗略的子力描述，用于说明净得失（如“约一个马”）。
pub fn material_phrase(value: i32) -> String {
    let v = value.abs();
    let text = match v {
        0..=60 => "很少",
        61..=150 => "约一个兵",
        151..=300 => "约一个仕象或两个兵",
        301..=500 => "约一个马或炮",
        501..=750 => "一个马炮以上",
        751..=1100 => "约一个车",
        _ => "超过一个车",
    };
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square { Square::parse(s).unwrap() }

    #[test]
    fn labels() {
        let red_horse = Piece::new(Color::Red, PieceKind::Horse);
        assert_eq!(piece_label(red_horse, sq("h0")), "右马");
        assert_eq!(piece_label(red_horse, sq("g2")), "三路马");
        let black_horse = Piece::new(Color::Black, PieceKind::Horse);
        assert_eq!(piece_label(black_horse, sq("h9")), "左马");
        assert_eq!(piece_label(Piece::new(Color::Red, PieceKind::Pawn), sq("e3")), "中兵");
        assert_eq!(piece_label(Piece::new(Color::Black, PieceKind::Pawn), sq("g6")), "7路卒");
        assert_eq!(piece_label(Piece::new(Color::Red, PieceKind::Cannon), sq("e2")), "中炮");
        assert_eq!(piece_label(Piece::new(Color::Red, PieceKind::Cannon), sq("h2")), "右炮");
        assert_eq!(join_labels(&["车".into(), "马".into(), "炮".into()]), "车、马和炮");
    }
}
