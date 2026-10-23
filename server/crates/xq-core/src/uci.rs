//! UCI 输出解析。评分区分普通分与将杀分，并保留上下界标记，避免把边界值当成确定值。

use std::cmp::Ordering;

use serde::Deserialize;
use serde::Serialize;

use crate::types::Move;

/// 从行棋方视角的评分。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Score {
    /// 引擎分（随 ScoreType 设置而定的刻度）。
    Cp(i32),
    /// 正数表示行棋方 N 步内将杀对方，负数表示被将杀；0 表示已被将死或困毙。
    Mate(i32),
}

impl Score {
    /// 统一排序键：越大越好。将杀分总在普通分之外，步数少的杀棋更好。
    pub fn order_key(self) -> i64 {
        match self {
            Score::Cp(cp) => cp as i64,
            Score::Mate(n) if n > 0 => 1_000_000 - n as i64,
            Score::Mate(n) => -1_000_000 - n as i64,
        }
    }

    pub fn cp(self) -> Option<i32> {
        match self {
            Score::Cp(cp) => Some(cp),
            Score::Mate(_) => None,
        }
    }

    pub fn is_mate(self) -> bool { matches!(self, Score::Mate(_)) }

    /// 换到对方视角。
    pub fn negate(self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(-cp),
            Score::Mate(n) => Score::Mate(-n),
        }
    }
}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Score {
    fn cmp(&self, other: &Self) -> Ordering { self.order_key().cmp(&other.order_key()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wdl {
    pub win: u32,
    pub draw: u32,
    pub loss: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InfoLine {
    pub depth: Option<u32>,
    pub seldepth: Option<u32>,
    pub multipv: Option<u32>,
    pub score: Option<Score>,
    pub bound: Option<Bound>,
    pub wdl: Option<Wdl>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time_ms: Option<u64>,
    pub hashfull: Option<u32>,
    pub pv: Vec<Move>,
    pub currmove: Option<Move>,
    pub string: Option<String>,
}

impl InfoLine {
    /// 带评分和主变化的完整搜索信息（区别于 currmove 进度和 info string）。
    pub fn is_search_line(&self) -> bool { self.depth.is_some() && self.score.is_some() }
}

fn next_num<'a, T: std::str::FromStr>(it: &mut impl Iterator<Item = &'a str>) -> Option<T> {
    it.next().and_then(|v| v.parse().ok())
}

/// 解析 `info ...` 行；非 info 行返回 None。
pub fn parse_info(line: &str) -> Option<InfoLine> {
    let mut it = line.split_whitespace();
    if it.next()? != "info" {
        return None;
    }
    let mut info = InfoLine::default();
    while let Some(key) = it.next() {
        match key {
            "depth" => info.depth = next_num(&mut it),
            "seldepth" => info.seldepth = next_num(&mut it),
            "multipv" => info.multipv = next_num(&mut it),
            "nodes" => info.nodes = next_num(&mut it),
            "nps" => info.nps = next_num(&mut it),
            "time" => info.time_ms = next_num(&mut it),
            "hashfull" => info.hashfull = next_num(&mut it),
            "tbhits" => {
                it.next();
            }
            "currmovenumber" => {
                it.next();
            }
            "currmove" => info.currmove = it.next().and_then(Move::parse_iccs),
            "score" => {
                match it.next() {
                    Some("cp") => info.score = next_num(&mut it).map(Score::Cp),
                    Some("mate") => info.score = next_num(&mut it).map(Score::Mate),
                    _ => {}
                }
                info.bound = Some(Bound::Exact);
            }
            "lowerbound" => info.bound = Some(Bound::Lower),
            "upperbound" => info.bound = Some(Bound::Upper),
            "wdl" => {
                let (w, d, l) = (next_num(&mut it), next_num(&mut it), next_num(&mut it));
                if let (Some(win), Some(draw), Some(loss)) = (w, d, l) {
                    info.wdl = Some(Wdl { win, draw, loss });
                }
            }
            "pv" => {
                for token in it.by_ref() {
                    match Move::parse_iccs(token) {
                        Some(mv) => info.pv.push(mv),
                        None => break,
                    }
                }
            }
            "string" => {
                let rest: Vec<&str> = it.by_ref().collect();
                info.string = Some(rest.join(" "));
            }
            _ => {}
        }
    }
    Some(info)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BestMove {
    Move { mv: Move, ponder: Option<Move> },
    /// `bestmove (none)`：行棋方没有合法着法。
    None,
}

pub fn parse_bestmove(line: &str) -> Option<BestMove> {
    let mut it = line.split_whitespace();
    if it.next()? != "bestmove" {
        return None;
    }
    let token = it.next()?;
    let Some(mv) = Move::parse_iccs(token) else { return Some(BestMove::None) };
    let ponder = match (it.next(), it.next()) {
        (Some("ponder"), Some(p)) => Move::parse_iccs(p),
        _ => None,
    };
    Some(BestMove::Move { mv, ponder })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UciOption {
    pub name: String,
    pub kind: String,
    pub default: Option<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub vars: Vec<String>,
}

/// 解析 `option name <名称> type <类型> [default ..] [min ..] [max ..] [var ..]*`。
pub fn parse_option(line: &str) -> Option<UciOption> {
    let rest = line.strip_prefix("option name ")?;
    let (name, rest) = rest.split_once(" type ")?;
    let mut tokens = rest.split_whitespace().peekable();
    let kind = tokens.next()?.to_string();
    let mut option =
        UciOption { name: name.trim().to_string(), kind, default: None, min: None, max: None, vars: Vec::new() };
    let keywords = ["default", "min", "max", "var"];
    while let Some(key) = tokens.next() {
        let mut value = Vec::new();
        while let Some(t) = tokens.peek() {
            if keywords.contains(t) {
                break;
            }
            value.push(tokens.next().expect("peek 已确认"));
        }
        let value = value.join(" ");
        match key {
            "default" => option.default = Some(value),
            "min" => option.min = value.parse().ok(),
            "max" => option.max = value.parse().ok(),
            "var" => option.vars.push(value),
            _ => {}
        }
    }
    Some(option)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pikafish_multipv_line() {
        let line = "info depth 8 seldepth 9 multipv 2 score cp 65 wdl 184 816 0 nodes 4023 nps 574714 hashfull 1 tbhits 0 time 7 pv b2e2 h9g7 b0c2";
        let info = parse_info(line).unwrap();
        assert_eq!(info.depth, Some(8));
        assert_eq!(info.multipv, Some(2));
        assert_eq!(info.score, Some(Score::Cp(65)));
        assert_eq!(info.bound, Some(Bound::Exact));
        assert_eq!(info.wdl, Some(Wdl { win: 184, draw: 816, loss: 0 }));
        assert_eq!(info.time_ms, Some(7));
        assert_eq!(info.pv.len(), 3);
        assert!(info.is_search_line());
    }

    #[test]
    fn parses_bounds_mate_and_strings() {
        let info = parse_info("info depth 20 score cp 31 lowerbound nodes 1 pv h2e2").unwrap();
        assert_eq!(info.bound, Some(Bound::Lower));
        let info = parse_info("info depth 0 score mate 0").unwrap();
        assert_eq!(info.score, Some(Score::Mate(0)));
        assert!(info.pv.is_empty());
        let info = parse_info("info string NNUE evaluation using pikafish.nnue").unwrap();
        assert!(!info.is_search_line());
        assert!(info.string.unwrap().contains("NNUE"));
        let info = parse_info("info depth 5 currmove h2e2 currmovenumber 1").unwrap();
        assert!(!info.is_search_line());
        assert!(parse_info("bestmove h2e2").is_none());
    }

    #[test]
    fn parses_bestmove_variants() {
        assert_eq!(
            parse_bestmove("bestmove h0i2 ponder c6c5"),
            Some(BestMove::Move { mv: Move::parse_iccs("h0i2").unwrap(), ponder: Move::parse_iccs("c6c5") })
        );
        assert_eq!(parse_bestmove("bestmove (none)"), Some(BestMove::None));
        assert_eq!(parse_bestmove("info depth 1"), None);
    }

    #[test]
    fn score_ordering() {
        assert!(Score::Mate(3) > Score::Mate(5));
        assert!(Score::Mate(5) > Score::Cp(5000));
        assert!(Score::Cp(-5000) > Score::Mate(-7));
        assert!(Score::Mate(-7) > Score::Mate(-2));
        assert!(Score::Mate(-2) > Score::Mate(0));
    }

    #[test]
    fn parses_options() {
        let o = parse_option("option name Threads type spin default 1 min 1 max 1024").unwrap();
        assert_eq!((o.name.as_str(), o.min, o.max), ("Threads", Some(1), Some(1024)));
        let o = parse_option(
            "option name Repetition Rule type combo default AsianRule var AsianRule var ChineseRule var SkyRule",
        )
        .unwrap();
        assert_eq!(o.name, "Repetition Rule");
        assert_eq!(o.vars, vec!["AsianRule", "ChineseRule", "SkyRule"]);
        let o = parse_option("option name Debug Log File type string default ").unwrap();
        assert_eq!(o.default.as_deref(), Some(""));
        let o = parse_option("option name Clear Hash type button").unwrap();
        assert_eq!(o.kind, "button");
    }
}
