//! 中文记谱：生成与解析。
//!
//! 规则要点：
//! - 红方纵线用中文数字，从红方右侧数起（i 线为一）；黑方用阿拉伯数字，从黑方右侧数起（a 线为 1）。
//! - 车、炮、兵、将帅直进直退记步数，平移记目标纵线；马、象、士记目标纵线。
//! - 同一纵线有两个同名棋子用“前/后”；兵卒有三个用“前/中/后”，四五个用“前、二、三、四、五”。
//! - 兵卒在两条以上纵线都有重叠时，省去“兵/卒”字，写作“前九平八”这样的形式。

use std::fmt;

use crate::board::Position;
use crate::rules;
use crate::types::Color;
use crate::types::Move;
use crate::types::PieceKind;
use crate::types::Square;

const CN_DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotationError {
    NoPiece(Square),
    Unrecognized(String),
    NoMatch(String),
    Ambiguous(String),
}

impl fmt::Display for NotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotationError::NoPiece(sq) => write!(f, "{sq} 没有棋子"),
            NotationError::Unrecognized(s) => write!(f, "无法识别的着法写法“{s}”"),
            NotationError::NoMatch(s) => write!(f, "当前局面没有与“{s}”对应的合法着法"),
            NotationError::Ambiguous(s) => write!(f, "“{s}”可以对应多步着法，请写明前/后"),
        }
    }
}

impl std::error::Error for NotationError {}

/// 纵线编号（从该方右侧数起）。
pub fn file_number(color: Color, file: u8) -> String {
    match color {
        Color::Red => CN_DIGITS[(9 - file) as usize].to_string(),
        Color::Black => (file + 1).to_string(),
    }
}

fn count_number(color: Color, n: u8) -> String {
    match color {
        Color::Red => CN_DIGITS[n as usize].to_string(),
        Color::Black => n.to_string(),
    }
}

/// 该方视角下 a 是否在 b 前面（更靠近对方底线）。
fn ahead(color: Color, a: Square, b: Square) -> bool {
    match color {
        Color::Red => a.rank() > b.rank(),
        Color::Black => a.rank() < b.rank(),
    }
}

/// 同一纵线上同名棋子从前到后的位置。
fn same_file_ordered(pos: &Position, color: Color, kind: PieceKind, file: u8) -> Vec<Square> {
    let mut squares: Vec<Square> = pos
        .board
        .pieces()
        .filter(|(sq, p)| p.color == color && p.kind == kind && sq.file() == file)
        .map(|(sq, _)| sq)
        .collect();
    squares.sort_by(|a, b| {
        if ahead(color, *a, *b) {
            std::cmp::Ordering::Less
        } else if ahead(color, *b, *a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    squares
}

fn order_marker(index: usize, total: usize) -> &'static str {
    match (total, index) {
        (2, 0) => "前",
        (2, _) => "后",
        (3, 0) => "前",
        (3, 1) => "中",
        (3, _) => "后",
        (_, 0) => "前",
        (_, 1) => "二",
        (_, 2) => "三",
        (_, 3) => "四",
        _ => "五",
    }
}

/// 生成中文记谱。只要求起点有棋子，不检查合法性。
pub fn move_to_chinese(pos: &Position, mv: Move) -> Result<String, NotationError> {
    let piece = pos.board.get(mv.from).ok_or(NotationError::NoPiece(mv.from))?;
    let color = piece.color;
    let same_file = same_file_ordered(pos, color, piece.kind, mv.from.file());
    let index = same_file.iter().position(|sq| *sq == mv.from).unwrap_or(0);

    let mut out = String::new();
    if same_file.len() <= 1 {
        out.push_str(piece.name());
        out.push_str(&file_number(color, mv.from.file()));
    } else {
        let tandem_files = if piece.kind == PieceKind::Pawn {
            (0..9u8).filter(|f| same_file_ordered(pos, color, PieceKind::Pawn, *f).len() >= 2).count()
        } else {
            1
        };
        out.push_str(order_marker(index, same_file.len()));
        if tandem_files >= 2 {
            out.push_str(&file_number(color, mv.from.file()));
        } else {
            out.push_str(piece.name());
        }
    }

    let forward = ahead(color, mv.to, mv.from);
    if mv.from.rank() == mv.to.rank() {
        out.push('平');
        out.push_str(&file_number(color, mv.to.file()));
    } else {
        out.push(if forward { '进' } else { '退' });
        if piece.kind.records_target_file() {
            out.push_str(&file_number(color, mv.to.file()));
        } else {
            let steps = (mv.to.rank() as i8 - mv.from.rank() as i8).unsigned_abs();
            out.push_str(&count_number(color, steps));
        }
    }
    Ok(out)
}

/// 依次生成一串着法的记谱；遇到起点无子时停止。
pub fn line_to_chinese(pos: &Position, moves: &[Move]) -> Vec<String> {
    let mut cur = pos.clone();
    let mut out = Vec::with_capacity(moves.len());
    for mv in moves {
        match move_to_chinese(&cur, *mv) {
            Ok(text) => out.push(text),
            Err(_) => break,
        }
        cur = cur.after(*mv);
    }
    out
}

/// 把不同写法统一成可比较的形式：棋子、方向、数字、前后标记。
fn normalize(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let mapped = match ch {
            '帅' | '帥' | '将' | '將' => 'K',
            '仕' | '士' => 'A',
            '相' | '象' => 'B',
            '马' | '馬' | '傌' | '碼' => 'N',
            '车' | '車' | '俥' => 'R',
            '炮' | '砲' | '包' => 'C',
            '兵' | '卒' => 'P',
            '进' | '進' => '+',
            '退' => '-',
            '平' => '=',
            '前' => 'f',
            '后' | '後' => 'b',
            '中' => 'm',
            '一' | '１' | '1' => '1',
            '二' | '２' | '2' => '2',
            '三' | '３' | '3' => '3',
            '四' | '４' | '4' => '4',
            '五' | '５' | '5' => '5',
            '六' | '６' | '6' => '6',
            '七' | '７' | '7' => '7',
            '八' | '８' | '8' => '8',
            '九' | '９' | '9' => '9',
            c if c.is_whitespace() => continue,
            _ => '?',
        };
        out.push(mapped);
    }
    out
}

/// 解析中文记谱：与当前局面所有合法着法的标准记谱比对；不完全规范的
/// “棋子+纵线+动作+数字”写法在唯一对应时也接受。
pub fn parse_chinese(pos: &Position, text: &str) -> Result<Move, NotationError> {
    let wanted = normalize(text);
    if wanted.chars().count() != 4 || wanted.contains('?') {
        return Err(NotationError::Unrecognized(text.to_string()));
    }
    let legal = rules::legal_moves(pos);
    let exact: Vec<Move> = legal
        .iter()
        .copied()
        .filter(|mv| move_to_chinese(pos, *mv).map(|s| normalize(&s) == wanted).unwrap_or(false))
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }

    // 宽松匹配：如“兵三进一”在有重叠兵时仍按纵线定位
    let chars: Vec<char> = wanted.chars().collect();
    let kind = match chars[0] {
        'K' => PieceKind::King,
        'A' => PieceKind::Advisor,
        'B' => PieceKind::Elephant,
        'N' => PieceKind::Horse,
        'R' => PieceKind::Rook,
        'C' => PieceKind::Cannon,
        'P' => PieceKind::Pawn,
        _ => return Err(NotationError::NoMatch(text.to_string())),
    };
    let Some(file_digit) = chars[1].to_digit(10) else { return Err(NotationError::NoMatch(text.to_string())) };
    let loose: Vec<Move> = legal
        .iter()
        .copied()
        .filter(|mv| {
            let Some(p) = pos.board.get(mv.from) else { return false };
            if p.kind != kind || file_number(p.color, mv.from.file()) != file_number_text(p.color, file_digit) {
                return false;
            }
            move_to_chinese(pos, *mv).map(|s| normalize(&s)[2..] == wanted[2..]).unwrap_or(false)
        })
        .collect();
    match loose.len() {
        1 => Ok(loose[0]),
        0 => Err(NotationError::NoMatch(text.to_string())),
        _ => Err(NotationError::Ambiguous(text.to_string())),
    }
}

fn file_number_text(color: Color, digit: u32) -> String {
    match color {
        Color::Red => CN_DIGITS[digit as usize].to_string(),
        Color::Black => digit.to_string(),
    }
}

/// 解析 ICCS（如 h2e2、h2-e2）或中文记谱。
pub fn parse_move(pos: &Position, text: &str) -> Result<Move, NotationError> {
    if let Some(mv) = Move::parse_iccs(text) {
        return if rules::is_legal(pos, mv) { Ok(mv) } else { Err(NotationError::NoMatch(text.to_string())) };
    }
    parse_chinese(pos, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_position;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    fn cn(fen: &str, m: &str) -> String { move_to_chinese(&parse_position(fen).unwrap(), mv(m)).unwrap() }

    #[test]
    fn opening_moves() {
        let start = crate::fen::START_FEN;
        assert_eq!(cn(start, "h2e2"), "炮二平五");
        assert_eq!(cn(start, "h0g2"), "马二进三");
        assert_eq!(cn(start, "h0i2"), "马二进一");
        assert_eq!(cn(start, "c0e2"), "相七进五");
        assert_eq!(cn(start, "f0e1"), "仕四进五");
        assert_eq!(cn(start, "i0i1"), "车一进一");
        assert_eq!(cn(start, "c3c4"), "兵七进一");
        let black = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR b";
        assert_eq!(cn(black, "h9g7"), "马8进7");
        assert_eq!(cn(black, "b7e7"), "炮2平5");
        assert_eq!(cn(black, "c9e7"), "象3进5");
        assert_eq!(cn(black, "a9a8"), "车1进1");
        assert_eq!(cn(black, "g6g5"), "卒7进1");
    }

    #[test]
    fn retreats_and_steps() {
        let fen = "4k4/9/9/9/9/2R6/9/9/9/4K4 w";
        assert_eq!(cn(fen, "c4c0"), "车七退四");
        assert_eq!(cn(fen, "c4c8"), "车七进四");
        let fen = "4k4/9/9/9/2r6/9/9/9/9/4K4 b";
        assert_eq!(cn(fen, "c5c1"), "车3进4");
        assert_eq!(cn(fen, "c5c9"), "车3退4");
    }

    #[test]
    fn front_and_back_pieces() {
        // 红方两车同在七路
        let fen = "4k4/9/9/9/9/2R6/9/2R6/9/4K4 w";
        assert_eq!(cn(fen, "c4d4"), "前车平六");
        assert_eq!(cn(fen, "c2d2"), "后车平六");
        // 黑方两马同在 3 路：黑方前方是 rank 小的一侧
        let fen = "4k4/9/2n6/9/2n6/9/9/9/9/4K4 b";
        assert_eq!(cn(fen, "c5d3"), "前马进4");
        assert_eq!(cn(fen, "c7b5"), "后马进2");
    }

    #[test]
    fn tandem_pawns_one_file() {
        // 黑方三个卒同在 3 路（c 线），从黑方看 rank 越小越靠前
        let fen = "4k4/9/9/9/9/2p6/2p6/2p6/9/3K5 b";
        assert_eq!(cn(fen, "c2c1"), "前卒进1");
        assert_eq!(cn(fen, "c3b3"), "中卒平2");
        assert_eq!(cn(fen, "c4b4"), "后卒平2");
        // 红方三个兵同在七路
        let fen = "4k4/9/2P6/2P6/2P6/9/9/9/9/4K4 w";
        assert_eq!(cn(fen, "c7c8"), "前兵进一");
        assert_eq!(cn(fen, "c6d6"), "中兵平六");
        assert_eq!(cn(fen, "c5b5"), "后兵平八");
        // 四个兵：前、二、三、四
        let fen = "4k4/2P6/2P6/2P6/2P6/9/9/9/9/3K5 w";
        assert_eq!(cn(fen, "c7d7"), "二兵平六");
        assert_eq!(cn(fen, "c6d6"), "三兵平六");
        assert_eq!(cn(fen, "c5d5"), "四兵平六");
    }

    #[test]
    fn tandem_pawns_two_files() {
        // 红方九路、七路各有两个兵：省去“兵”字，写纵线
        let fen = "4k4/9/2P5P/2P5P/9/9/9/9/9/3K5 w";
        assert_eq!(cn(fen, "i7h7"), "前一平二");
        assert_eq!(cn(fen, "c6d6"), "后七平六");
    }

    #[test]
    fn parse_roundtrip_all_legal_moves() {
        for fen in [
            crate::fen::START_FEN,
            "4k4/9/2P5P/2P5P/9/9/9/9/9/3K5 w",
            "4k4/9/9/9/9/2p6/2p6/2p6/9/3K5 b",
            "4k4/2P6/2P6/2P6/2P6/9/9/9/9/3K5 w",
            "2bak4/4a4/2n1b4/p1p1C1p2/9/2P6/P3r1P1P/4B1N2/4A4/2BAK4 w",
        ] {
            let pos = parse_position(fen).unwrap();
            for m in rules::legal_moves(&pos) {
                let text = move_to_chinese(&pos, m).unwrap();
                assert_eq!(parse_chinese(&pos, &text), Ok(m), "{fen} {text}");
            }
        }
    }

    #[test]
    fn parse_variant_spellings() {
        let pos = Position::startpos();
        assert_eq!(parse_move(&pos, "炮二平五"), Ok(mv("h2e2")));
        assert_eq!(parse_move(&pos, "炮2平5"), Ok(mv("h2e2")));
        assert_eq!(parse_move(&pos, "h2-e2"), Ok(mv("h2e2")));
        assert_eq!(parse_move(&pos, "傌二進三"), Ok(mv("h0g2")));
        assert!(matches!(parse_move(&pos, "炮二平十"), Err(NotationError::Unrecognized(_))));
        assert!(matches!(parse_move(&pos, "车一平二"), Err(NotationError::NoMatch(_))));
        let black = parse_position("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR b").unwrap();
        assert_eq!(parse_move(&black, "马８进７"), Ok(mv("h9g7")));
    }
}
