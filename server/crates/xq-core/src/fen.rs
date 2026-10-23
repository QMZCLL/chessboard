use std::fmt;

use crate::board::Board;
use crate::board::Position;
use crate::types::Color;
use crate::types::Piece;
use crate::types::Square;

/// FEN 格式错误。只描述格式问题，棋子是否合理由 `validate` 负责。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenError {
    Empty,
    RankCount(usize),
    RankWidth { row: usize, width: usize },
    BadChar { row: usize, ch: char },
    MissingSide,
    BadSide(String),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenError::Empty => write!(f, "FEN 为空"),
            FenError::RankCount(n) => write!(f, "FEN 应有 10 行（用 / 分隔），实际为 {n} 行"),
            FenError::RankWidth { row, width } => {
                write!(f, "FEN 第 {} 行应正好 9 列，实际为 {width} 列", row + 1)
            }
            FenError::BadChar { row, ch } => write!(f, "FEN 第 {} 行含无法识别的字符 '{ch}'", row + 1),
            FenError::MissingSide => write!(f, "FEN 缺少行棋方（w 表示红走，b 表示黑走）"),
            FenError::BadSide(s) => write!(f, "无法识别的行棋方 '{s}'，应为 w（红）或 b（黑）"),
        }
    }
}

impl std::error::Error for FenError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFen {
    pub board: Board,
    pub side: Option<Color>,
    /// FEN 末尾的计数字段（未吃子半回合数、回合数），给出时保留，涉及限着规则时引擎需要它们。
    pub counters: Option<(u32, u32)>,
}

pub fn parse_side(s: &str) -> Result<Color, FenError> {
    match s {
        "w" | "W" | "r" | "R" | "red" => Ok(Color::Red),
        "b" | "B" | "black" => Ok(Color::Black),
        other => Err(FenError::BadSide(other.to_string())),
    }
}

/// 解析棋子布局、可选的行棋方与可选的计数字段（`- -` 占位字段被跳过）。
pub fn parse_fen(text: &str) -> Result<ParsedFen, FenError> {
    let mut fields = text.split_whitespace();
    let placement = fields.next().ok_or(FenError::Empty)?;
    let rows: Vec<&str> = placement.split('/').collect();
    if rows.len() != 10 {
        return Err(FenError::RankCount(rows.len()));
    }
    let mut board = Board::empty();
    for (row, text) in rows.iter().enumerate() {
        let rank = 9 - row as u8;
        let mut file = 0usize;
        for ch in text.chars() {
            if let Some(n) = ch.to_digit(10) {
                if n == 0 {
                    return Err(FenError::BadChar { row, ch });
                }
                file += n as usize;
            } else if let Some(piece) = Piece::from_fen_char(ch) {
                if file < 9 {
                    board.set(Square::new(file as u8, rank).expect("file 与 rank 已检查"), Some(piece));
                }
                file += 1;
            } else {
                return Err(FenError::BadChar { row, ch });
            }
        }
        if file != 9 {
            return Err(FenError::RankWidth { row, width: file });
        }
    }
    let side = match fields.next() {
        Some(s) => Some(parse_side(s)?),
        None => None,
    };
    let numbers: Vec<u32> = fields.filter(|f| *f != "-").filter_map(|f| f.parse().ok()).collect();
    let counters = match numbers.as_slice() {
        [half, full, ..] => Some((*half, *full)),
        _ => None,
    };
    Ok(ParsedFen { board, side, counters })
}

/// 解析完整局面，必须给出行棋方。
pub fn parse_position(text: &str) -> Result<Position, FenError> {
    let parsed = parse_fen(text)?;
    let side = parsed.side.ok_or(FenError::MissingSide)?;
    Ok(Position::new(parsed.board, side))
}

pub fn board_fen(board: &Board) -> String {
    let mut out = String::with_capacity(64);
    for rank in (0..10u8).rev() {
        let mut empty = 0;
        for file in 0..9u8 {
            match board.get(Square::new(file, rank).expect("坐标在棋盘内")) {
                Some(p) => {
                    if empty > 0 {
                        out.push_str(&empty.to_string());
                        empty = 0;
                    }
                    out.push(p.fen_char());
                }
                None => empty += 1,
            }
        }
        if empty > 0 {
            out.push_str(&empty.to_string());
        }
        if rank > 0 {
            out.push('/');
        }
    }
    out
}

pub fn position_fen(pos: &Position) -> String { format!("{} {}", board_fen(&pos.board), pos.side.fen_char()) }

/// 带计数字段的 FEN；没有计数字段时与 `position_fen` 相同。
pub fn position_fen_with_counters(pos: &Position, counters: Option<(u32, u32)>) -> String {
    match counters {
        Some((half, full)) => format!("{} - - {half} {full}", position_fen(pos)),
        None => position_fen(pos),
    }
}

/// 只含棋子布局与行棋方的部分，用于比较两个 FEN 是否同一局面。
pub fn position_key(fen: &str) -> String { fen.split_whitespace().take(2).collect::<Vec<_>>().join(" ") }

pub const START_FEN: &str = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_roundtrip() {
        let pos = parse_position(START_FEN).unwrap();
        assert_eq!(pos, Position::startpos());
        assert_eq!(position_fen(&pos), START_FEN);
    }

    #[test]
    fn counters_are_kept() {
        let parsed = parse_fen("4k4/9/9/9/9/9/9/9/9/3K5 b - - 12 40").unwrap();
        assert_eq!(parsed.side, Some(Color::Black));
        assert_eq!(parsed.counters, Some((12, 40)));
        assert_eq!(parse_fen("4k4/9/9/9/9/9/9/9/9/3K5 w 3 7").unwrap().counters, Some((3, 7)));
        assert_eq!(parse_fen("4k4/9/9/9/9/9/9/9/9/3K5 w").unwrap().counters, None);
        let pos = parse_position("4k4/9/9/9/9/9/9/9/9/3K5 b - - 12 40").unwrap();
        let text = position_fen_with_counters(&pos, Some((12, 40)));
        assert_eq!(text, "4k4/9/9/9/9/9/9/9/9/3K5 b - - 12 40");
        assert_eq!(position_key(&text), position_fen(&pos));
    }

    #[test]
    fn format_errors() {
        assert_eq!(parse_fen(""), Err(FenError::Empty));
        assert_eq!(parse_fen("9/9/9"), Err(FenError::RankCount(3)));
        assert_eq!(parse_fen("4k5/9/9/9/9/9/9/9/9/4K4 w"), Err(FenError::RankWidth { row: 0, width: 10 }));
        assert_eq!(parse_fen("4x4/9/9/9/9/9/9/9/9/4K4 w"), Err(FenError::BadChar { row: 0, ch: 'x' }));
        assert_eq!(parse_position("4k4/9/9/9/9/9/9/9/9/4K4"), Err(FenError::MissingSide));
        assert!(matches!(parse_position("4k4/9/9/9/9/9/9/9/9/4K4 x"), Err(FenError::BadSide(_))));
    }
}
