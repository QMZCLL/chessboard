//! 象棋云库（chessdb.cn `querypv`）响应解析。云库只作为可选参考来源，
//! 其分数不与本地引擎分数直接相减。

use serde::Deserialize;
use serde::Serialize;

use crate::types::Move;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudOutcome {
    Moves { score: i32, depth: u32, pv: Vec<Move> },
    /// 云库没有该局面的数据。
    Unknown,
    /// 云库认为局面不成立，需要检查局面本身。
    InvalidBoard,
    Checkmate,
    Stalemate,
}

/// 解析响应正文。格式不符时返回错误说明，而不是猜测。
pub fn parse_querypv(text: &str) -> Result<CloudOutcome, String> {
    let body = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    match body {
        "" | "unknown" | "nobestmove" => return Ok(CloudOutcome::Unknown),
        "invalid board" => return Ok(CloudOutcome::InvalidBoard),
        "checkmate" => return Ok(CloudOutcome::Checkmate),
        "stalemate" => return Ok(CloudOutcome::Stalemate),
        _ => {}
    }
    let mut score = None;
    let mut depth = None;
    let mut pv = Vec::new();
    for pair in body.split(',') {
        let Some((key, value)) = pair.split_once(':') else {
            return Err(format!("云库响应格式无法识别：{}", truncate(body)));
        };
        match key.trim() {
            "score" => score = value.trim().parse::<i32>().ok(),
            "depth" => depth = value.trim().parse::<u32>().ok(),
            "pv" => {
                for token in value.split('|') {
                    match Move::parse_iccs(token) {
                        Some(mv) => pv.push(mv),
                        None => return Err(format!("云库返回了无法识别的着法“{}”", truncate(token))),
                    }
                }
            }
            _ => {}
        }
    }
    match (score, pv.is_empty()) {
        (Some(score), false) => Ok(CloudOutcome::Moves { score, depth: depth.unwrap_or(0), pv }),
        _ => Err(format!("云库响应缺少分数或着法：{}", truncate(body))),
    }
}

fn truncate(s: &str) -> String { s.chars().take(80).collect() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_shapes() {
        let r = parse_querypv("score:28,depth:21,pv:h2e2|h9g7|h0g2\0").unwrap();
        assert_eq!(r, CloudOutcome::Moves { score: 28, depth: 21, pv: vec![
            Move::parse_iccs("h2e2").unwrap(),
            Move::parse_iccs("h9g7").unwrap(),
            Move::parse_iccs("h0g2").unwrap()
        ] });
        assert_eq!(parse_querypv("unknown\0").unwrap(), CloudOutcome::Unknown);
        assert_eq!(parse_querypv("checkmate").unwrap(), CloudOutcome::Checkmate);
        assert_eq!(parse_querypv("stalemate\0").unwrap(), CloudOutcome::Stalemate);
        assert_eq!(parse_querypv("invalid board").unwrap(), CloudOutcome::InvalidBoard);
    }

    #[test]
    fn rejects_unexpected_bodies() {
        assert!(parse_querypv("<html>502 Bad Gateway</html>").is_err());
        assert!(parse_querypv("score:abc,pv:").is_err());
        assert!(parse_querypv("score:1,depth:2,pv:zz99").is_err());
    }
}
