use std::fmt;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    Red,
    Black,
}

impl Color {
    pub fn opposite(self) -> Color {
        match self {
            Color::Red => Color::Black,
            Color::Black => Color::Red,
        }
    }

    pub fn fen_char(self) -> char {
        match self {
            Color::Red => 'w',
            Color::Black => 'b',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Color::Red => "红方",
            Color::Black => "黑方",
        }
    }

    /// 向前走时 rank 的变化方向。
    pub fn forward(self) -> i8 {
        match self {
            Color::Red => 1,
            Color::Black => -1,
        }
    }

    pub fn home_rank(self) -> u8 {
        match self {
            Color::Red => 0,
            Color::Black => 9,
        }
    }

    /// rank 是否位于本方半场（未过河）。
    pub fn own_half(self, rank: u8) -> bool {
        match self {
            Color::Red => rank <= 4,
            Color::Black => rank >= 5,
        }
    }

    pub fn in_palace(self, sq: Square) -> bool {
        let ranks = match self {
            Color::Red => 0..=2,
            Color::Black => 7..=9,
        };
        (3..=5).contains(&sq.file()) && ranks.contains(&sq.rank())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PieceKind {
    King,
    Advisor,
    Elephant,
    Horse,
    Rook,
    Cannon,
    Pawn,
}

impl PieceKind {
    pub const ALL: [PieceKind; 7] = [
        PieceKind::King,
        PieceKind::Advisor,
        PieceKind::Elephant,
        PieceKind::Horse,
        PieceKind::Rook,
        PieceKind::Cannon,
        PieceKind::Pawn,
    ];

    pub fn max_count(self) -> usize {
        match self {
            PieceKind::King => 1,
            PieceKind::Pawn => 5,
            _ => 2,
        }
    }

    /// 马、象、士按落点纵线记谱，其余直走棋子按步数记谱。
    pub fn records_target_file(self) -> bool {
        matches!(self, PieceKind::Horse | PieceKind::Elephant | PieceKind::Advisor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

impl Piece {
    pub const fn new(color: Color, kind: PieceKind) -> Self { Self { color, kind } }

    /// 接受常见 FEN 字母；象和马也接受 E/H 写法。
    pub fn from_fen_char(c: char) -> Option<Piece> {
        let color = if c.is_ascii_uppercase() { Color::Red } else { Color::Black };
        let kind = match c.to_ascii_lowercase() {
            'k' => PieceKind::King,
            'a' => PieceKind::Advisor,
            'b' | 'e' => PieceKind::Elephant,
            'n' | 'h' => PieceKind::Horse,
            'r' => PieceKind::Rook,
            'c' => PieceKind::Cannon,
            'p' => PieceKind::Pawn,
            _ => return None,
        };
        Some(Piece { color, kind })
    }

    pub fn fen_char(self) -> char {
        let c = match self.kind {
            PieceKind::King => 'k',
            PieceKind::Advisor => 'a',
            PieceKind::Elephant => 'b',
            PieceKind::Horse => 'n',
            PieceKind::Rook => 'r',
            PieceKind::Cannon => 'c',
            PieceKind::Pawn => 'p',
        };
        match self.color {
            Color::Red => c.to_ascii_uppercase(),
            Color::Black => c,
        }
    }

    pub fn name(self) -> &'static str {
        match (self.color, self.kind) {
            (Color::Red, PieceKind::King) => "帅",
            (Color::Black, PieceKind::King) => "将",
            (Color::Red, PieceKind::Advisor) => "仕",
            (Color::Black, PieceKind::Advisor) => "士",
            (Color::Red, PieceKind::Elephant) => "相",
            (Color::Black, PieceKind::Elephant) => "象",
            (_, PieceKind::Horse) => "马",
            (_, PieceKind::Rook) => "车",
            (_, PieceKind::Cannon) => "炮",
            (Color::Red, PieceKind::Pawn) => "兵",
            (Color::Black, PieceKind::Pawn) => "卒",
        }
    }

    /// 讲解用的粗略子力价值（兵过河后价值上升）。
    pub fn value_at(self, sq: Square) -> i32 {
        match self.kind {
            PieceKind::King => 10_000,
            PieceKind::Rook => 900,
            PieceKind::Cannon => 450,
            PieceKind::Horse => 400,
            PieceKind::Advisor | PieceKind::Elephant => 200,
            PieceKind::Pawn => {
                if self.color.own_half(sq.rank()) {
                    100
                } else {
                    200
                }
            }
        }
    }
}

impl Serialize for Piece {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_char(self.fen_char()) }
}

impl<'de> Deserialize<'de> for Piece {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let c = char::deserialize(d)?;
        Piece::from_fen_char(c).ok_or_else(|| serde::de::Error::custom(format!("未知棋子编码 '{c}'")))
    }
}

/// 棋盘交叉点，`index = rank * 9 + file`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Square(u8);

impl Square {
    pub fn new(file: u8, rank: u8) -> Option<Square> {
        if file < 9 && rank < 10 { Some(Square(rank * 9 + file)) } else { None }
    }

    pub fn from_index(index: usize) -> Option<Square> { if index < 90 { Some(Square(index as u8)) } else { None } }

    pub fn index(self) -> usize { self.0 as usize }

    pub fn file(self) -> u8 { self.0 % 9 }

    pub fn rank(self) -> u8 { self.0 / 9 }

    pub fn offset(self, dfile: i8, drank: i8) -> Option<Square> {
        let file = self.file() as i8 + dfile;
        let rank = self.rank() as i8 + drank;
        if (0..9).contains(&file) && (0..10).contains(&rank) { Square::new(file as u8, rank as u8) } else { None }
    }

    pub fn all() -> impl Iterator<Item = Square> { (0..90u8).map(Square) }

    pub fn iccs(self) -> String { format!("{}{}", (b'a' + self.file()) as char, self.rank()) }

    pub fn parse(s: &str) -> Option<Square> {
        let mut chars = s.chars();
        let file = chars.next()?.to_ascii_lowercase();
        let rank = chars.next()?;
        if chars.next().is_some() || !('a'..='i').contains(&file) || !rank.is_ascii_digit() {
            return None;
        }
        Square::new(file as u8 - b'a', rank as u8 - b'0')
    }

    /// 180° 旋转后的同一交叉点，用于黑方在下的画面换算。
    pub fn rotated(self) -> Square { Square(89 - self.0) }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.iccs()) }
}

impl Serialize for Square {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(&self.iccs()) }
}

impl<'de> Deserialize<'de> for Square {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Square::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("无效坐标 '{s}'")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Move {
    pub from: Square,
    pub to: Square,
}

impl Move {
    pub fn new(from: Square, to: Square) -> Self { Self { from, to } }

    pub fn iccs(self) -> String { format!("{}{}", self.from.iccs(), self.to.iccs()) }

    /// 解析 `h2e2`、`h2-e2`、`H2-E2` 等写法。
    pub fn parse_iccs(s: &str) -> Option<Move> {
        let cleaned: String = s.trim().chars().filter(|c| *c != '-').collect();
        if cleaned.len() != 4 || !cleaned.is_ascii() {
            return None;
        }
        let from = Square::parse(&cleaned[0..2])?;
        let to = Square::parse(&cleaned[2..4])?;
        if from == to {
            return None;
        }
        Some(Move { from, to })
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.iccs()) }
}

impl Serialize for Move {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(&self.iccs()) }
}

impl<'de> Deserialize<'de> for Move {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Move::parse_iccs(&s).ok_or_else(|| serde::de::Error::custom(format!("无效走法 '{s}'")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_roundtrip() {
        for sq in Square::all() {
            assert_eq!(Square::parse(&sq.iccs()), Some(sq));
            assert_eq!(sq.rotated().rotated(), sq);
        }
        assert_eq!(Square::parse("e0").unwrap().rotated(), Square::parse("e9").unwrap());
        assert_eq!(Square::parse("j0"), None);
        assert_eq!(Square::parse("a10"), None);
    }

    #[test]
    fn move_parse_variants() {
        let mv = Move::parse_iccs("h2e2").unwrap();
        assert_eq!(Move::parse_iccs("H2-E2"), Some(mv));
        assert_eq!(mv.iccs(), "h2e2");
        assert_eq!(Move::parse_iccs("h2h2"), None);
        assert_eq!(Move::parse_iccs("炮二平五"), None);
    }

    #[test]
    fn piece_codes() {
        assert_eq!(Piece::from_fen_char('E'), Some(Piece::new(Color::Red, PieceKind::Elephant)));
        assert_eq!(Piece::from_fen_char('h').unwrap().fen_char(), 'n');
        assert_eq!(Piece::from_fen_char('x'), None);
    }
}
