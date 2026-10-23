//! 棋谱导入：支持 PGN 风格标签、`[FEN "..."]` 起始局面、`fen ... moves ...` 形式，
//! 着法可以是 ICCS（h2e2 / h2-e2）或中文记谱。遇到无法解析的着法时停止，
//! 并说明停在第几步，已解析部分仍可使用。

use std::fmt;

use serde::Serialize;

use crate::board::Position;
use crate::fen;
use crate::notation;
use crate::types::Move;
use crate::validate;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordStop {
    /// 第几个半回合（从 1 开始）。
    pub ply: usize,
    pub token: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct ParsedRecord {
    pub start: Position,
    /// 起始局面是否来自棋谱中的 FEN（否则为标准开局）。
    pub custom_start: bool,
    pub moves: Vec<Move>,
    pub tags: Vec<(String, String)>,
    pub stopped: Option<RecordStop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordError {
    Empty,
    BadFen(String),
    InvalidStart(String),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::Empty => write!(f, "没有找到着法或起始局面"),
            RecordError::BadFen(e) => write!(f, "起始局面 FEN 有误：{e}"),
            RecordError::InvalidStart(e) => write!(f, "起始局面不成立：{e}"),
        }
    }
}

impl std::error::Error for RecordError {}

/// 去掉 `{注释}`、`(变着)` 与 `;` 行注释。
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut brace = 0usize;
    let mut paren = 0usize;
    let mut line_comment = false;
    for ch in text.chars() {
        if line_comment {
            if ch == '\n' {
                line_comment = false;
                out.push('\n');
            }
            continue;
        }
        match ch {
            '{' => brace += 1,
            '}' if brace > 0 => brace -= 1,
            '(' | '（' if brace == 0 => paren += 1,
            ')' | '）' if brace == 0 && paren > 0 => paren -= 1,
            ';' if brace == 0 && paren == 0 => line_comment = true,
            _ if brace == 0 && paren == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

fn is_result(token: &str) -> bool {
    matches!(token, "1-0" | "0-1" | "1/2-1/2" | "*" | "红胜" | "黑胜" | "和棋" | "红先胜" | "黑先胜")
}

/// 去掉 `12.`、`12...`、`12、` 这样的回合编号前缀。
fn strip_move_number(token: &str) -> &str {
    let digits = token.find(|c: char| !c.is_ascii_digit()).unwrap_or(token.len());
    if digits == 0 {
        return token;
    }
    let rest = &token[digits..];
    let trimmed = rest.trim_start_matches(['.', '…', '、', ')']);
    if trimmed.len() == rest.len() { token } else { trimmed }
}

fn is_cjk_move_char(c: char) -> bool { !c.is_ascii() || c.is_ascii_digit() }

/// 把一段 token 拆成单步着法：未加空格的中文记谱按 4 字切分。
fn split_moves(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() > 4 && chars.len().is_multiple_of(4) && chars.iter().all(|c| is_cjk_move_char(*c)) {
        chars.chunks(4).map(|c| c.iter().collect()).collect()
    } else {
        vec![token.to_string()]
    }
}

fn parse_tag(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (key, rest) = inner.split_once(char::is_whitespace)?;
    let value = rest.trim().trim_matches('"');
    Some((key.to_string(), value.to_string()))
}

fn looks_like_fen(text: &str) -> bool { text.split_whitespace().next().is_some_and(|f| f.matches('/').count() == 9) }

pub fn parse_record(text: &str) -> Result<ParsedRecord, RecordError> {
    let mut tags = Vec::new();
    let mut body = String::new();
    for line in text.lines() {
        if let Some(tag) = parse_tag(line) {
            tags.push(tag);
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }

    let mut start_fen: Option<String> =
        tags.iter().find(|(k, _)| k.eq_ignore_ascii_case("FEN")).map(|(_, v)| v.clone());
    let mut body = strip_comments(&body).trim().to_string();

    // `position fen <FEN> moves ...` 或 `<FEN> moves ...`
    let lowered = body.trim_start_matches("position").trim();
    let lowered = lowered.strip_prefix("fen").map(str::trim).unwrap_or(lowered);
    if looks_like_fen(lowered) {
        let (fen_part, moves_part) = match lowered.split_once("moves") {
            Some((f, m)) => (f.trim().to_string(), m.trim().to_string()),
            None => (lowered.to_string(), String::new()),
        };
        start_fen = Some(fen_part);
        body = moves_part;
    } else if body.starts_with("position startpos") {
        body = body.split_once("moves").map(|(_, m)| m.trim().to_string()).unwrap_or_default();
    }

    let custom_start = start_fen.is_some();
    let start = match &start_fen {
        Some(f) => {
            let parsed = fen::parse_fen(f).map_err(|e| RecordError::BadFen(e.to_string()))?;
            Position::new(parsed.board, parsed.side.unwrap_or(crate::types::Color::Red))
        }
        None => Position::startpos(),
    };
    let report = validate::validate_position(&start);
    if !report.ok {
        let reasons: Vec<String> = report.issues.iter().map(|i| i.message.clone()).collect();
        return Err(RecordError::InvalidStart(reasons.join("；")));
    }

    let tokens: Vec<String> = body
        .split(|c: char| c.is_whitespace() || c == ',' || c == '，')
        .map(strip_move_number)
        .filter(|t| !t.is_empty() && !is_result(t))
        .flat_map(split_moves)
        .collect();

    let mut pos = start.clone();
    let mut moves = Vec::new();
    let mut stopped = None;
    for token in tokens {
        match notation::parse_move(&pos, &token) {
            Ok(mv) => {
                pos = pos.after(mv);
                moves.push(mv);
            }
            Err(e) => {
                stopped = Some(RecordStop { ply: moves.len() + 1, token, reason: e.to_string() });
                break;
            }
        }
    }
    if moves.is_empty() && !custom_start && stopped.is_none() {
        return Err(RecordError::Empty);
    }
    Ok(ParsedRecord { start, custom_start, moves, tags, stopped })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iccs(r: &ParsedRecord) -> Vec<String> { r.moves.iter().map(|m| m.iccs()).collect() }

    #[test]
    fn chinese_record_with_numbers_and_comments() {
        let text = "[Event \"测试\"]\n1. 炮二平五 马８进７ {常见应法}\n2. 马二进三 车９平８ (2... 卒７进１) 3. 车一平二 1-0";
        let r = parse_record(text).unwrap();
        assert_eq!(iccs(&r), vec!["h2e2", "h9g7", "h0g2", "i9h9", "i0h0"]);
        assert!(r.stopped.is_none());
        assert_eq!(r.tags[0], ("Event".to_string(), "测试".to_string()));
    }

    #[test]
    fn unspaced_chinese_and_iccs() {
        let r = parse_record("炮二平五马8进7马二进三").unwrap();
        assert_eq!(iccs(&r), vec!["h2e2", "h9g7", "h0g2"]);
        let r = parse_record("h2-e2 h9-g7 h0-g2").unwrap();
        assert_eq!(iccs(&r), vec!["h2e2", "h9g7", "h0g2"]);
    }

    #[test]
    fn fen_with_moves() {
        let r = parse_record("position fen 3k5/9/9/9/9/9/9/9/4R4/4K4 w moves e1d1").unwrap();
        assert!(r.custom_start);
        assert_eq!(iccs(&r), vec!["e1d1"]);
        let r = parse_record("[FEN \"3k5/9/9/9/9/9/9/9/4R4/4K4 w\"]\n1. 车五平六").unwrap();
        assert_eq!(iccs(&r), vec!["e1d1"]);
    }

    #[test]
    fn stops_at_bad_move() {
        let r = parse_record("1. 炮二平五 马8进7 2. 车一平二").unwrap();
        assert_eq!(r.moves.len(), 2);
        let stop = r.stopped.unwrap();
        assert_eq!(stop.ply, 3);
        assert_eq!(stop.token, "车一平二");
    }

    #[test]
    fn rejects_invalid_start() {
        assert!(matches!(parse_record("[FEN \"4k4/9/9/9/9/9/9/9/9/4K4 w\"]"), Err(RecordError::InvalidStart(_))));
        assert_eq!(parse_record("   ").unwrap_err(), RecordError::Empty);
    }
}
