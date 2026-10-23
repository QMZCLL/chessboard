use std::fmt;

use crate::types::Color;
use crate::types::Move;
use crate::types::Piece;
use crate::types::PieceKind;
use crate::types::Square;

/// 标准方向（红方在下）的棋子布局。
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Board {
    cells: [Option<Piece>; 90],
}

impl Board {
    pub fn empty() -> Self { Self { cells: [None; 90] } }

    pub fn startpos() -> Self {
        let mut board = Board::empty();
        let back = [
            PieceKind::Rook,
            PieceKind::Horse,
            PieceKind::Elephant,
            PieceKind::Advisor,
            PieceKind::King,
            PieceKind::Advisor,
            PieceKind::Elephant,
            PieceKind::Horse,
            PieceKind::Rook,
        ];
        for (file, kind) in back.iter().enumerate() {
            board.put(file as u8, 0, Piece::new(Color::Red, *kind));
            board.put(file as u8, 9, Piece::new(Color::Black, *kind));
        }
        for file in [1u8, 7] {
            board.put(file, 2, Piece::new(Color::Red, PieceKind::Cannon));
            board.put(file, 7, Piece::new(Color::Black, PieceKind::Cannon));
        }
        for file in [0u8, 2, 4, 6, 8] {
            board.put(file, 3, Piece::new(Color::Red, PieceKind::Pawn));
            board.put(file, 6, Piece::new(Color::Black, PieceKind::Pawn));
        }
        board
    }

    fn put(&mut self, file: u8, rank: u8, piece: Piece) {
        if let Some(sq) = Square::new(file, rank) {
            self.cells[sq.index()] = Some(piece);
        }
    }

    pub fn get(&self, sq: Square) -> Option<Piece> { self.cells[sq.index()] }

    pub fn set(&mut self, sq: Square, piece: Option<Piece>) { self.cells[sq.index()] = piece; }

    pub fn pieces(&self) -> impl Iterator<Item = (Square, Piece)> + '_ {
        Square::all().filter_map(|sq| self.get(sq).map(|p| (sq, p)))
    }

    pub fn find_king(&self, color: Color) -> Option<Square> {
        self.pieces().find(|(_, p)| p.color == color && p.kind == PieceKind::King).map(|(sq, _)| sq)
    }

    pub fn count(&self, piece: Piece) -> usize { self.pieces().filter(|(_, p)| *p == piece).count() }

    /// 执行走子并返回被吃的棋子；不检查规则。
    pub fn apply(&mut self, mv: Move) -> Option<Piece> {
        let moving = self.get(mv.from);
        let captured = self.get(mv.to);
        self.set(mv.to, moving);
        self.set(mv.from, None);
        captured
    }

    pub fn applied(&self, mv: Move) -> Board {
        let mut next = self.clone();
        next.apply(mv);
        next
    }

    /// 旋转 180°，用于黑方在下的画面与标准方向之间的换算。
    pub fn rotated(&self) -> Board {
        let mut out = Board::empty();
        for (sq, p) in self.pieces() {
            out.set(sq.rotated(), Some(p));
        }
        out
    }

    pub fn is_empty(&self) -> bool { self.cells.iter().all(Option::is_none) }
}

impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in (0..10u8).rev() {
            for file in 0..9u8 {
                let c = Square::new(file, rank).and_then(|sq| self.get(sq)).map(Piece::fen_char).unwrap_or('.');
                write!(f, "{c}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// 棋子布局加当前行棋方。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Position {
    pub board: Board,
    pub side: Color,
}

impl Position {
    pub fn new(board: Board, side: Color) -> Self { Self { board, side } }

    pub fn startpos() -> Self { Self { board: Board::startpos(), side: Color::Red } }

    /// 走子后的局面（行棋方交换）；不检查规则。
    pub fn after(&self, mv: Move) -> Position { Position { board: self.board.applied(mv), side: self.side.opposite() } }

    pub fn is_startpos(&self) -> bool { self.side == Color::Red && self.board == Board::startpos() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_material() {
        let b = Board::startpos();
        assert_eq!(b.pieces().count(), 32);
        assert_eq!(b.find_king(Color::Red), Square::parse("e0"));
        assert_eq!(b.find_king(Color::Black), Square::parse("e9"));
        assert_eq!(b.rotated().rotated(), b);
    }

    #[test]
    fn apply_capture() {
        let mut b = Board::startpos();
        let captured = b.apply(Move::parse_iccs("b2b9").unwrap());
        assert_eq!(captured.map(|p| p.fen_char()), Some('n'));
        assert_eq!(b.get(Square::parse("b2").unwrap()), None);
    }
}
