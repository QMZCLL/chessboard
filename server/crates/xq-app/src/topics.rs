//! 专题：复习卡片的局面主题（程序根据这一着的规则事实建议）与用户自己确认的问题。
//!
//! - 主题只来自规则事实（应将与将军、受攻击的棋子、保护关系、线路与炮架、吃子与回吃），每个主题附上依据，
//!   不推测用户当时的想法；
//! - 问题由用户自己标记（如“没有检查对方的直接威胁”），随时可以修改；
//! - 复习列表可以按主题或问题练一组，界面据此推荐练习并说明依据。

use serde::Deserialize;
use serde::Serialize;
use xq_core::Board;
use xq_core::Move;
use xq_core::Square;
use xq_core::explain::describe::join_labels;
use xq_core::explain::describe::label_at;
use xq_core::explain::facts;
use xq_core::fen;

/// 局面主题。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// 应将与将帅安全。
    KingSafety,
    /// 受攻击棋子的处理。
    Threatened,
    /// 保护关系变化。
    Protection,
    /// 炮架与线路变化。
    Lines,
    /// 吃子后的回吃关系。
    Recapture,
}

/// 主题与它的依据（规则事实）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ThemeTag {
    pub theme: Theme,
    pub why: String,
}

/// 用户自己确认的问题。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Issue {
    /// 没有检查对方的直接威胁。
    MissedThreat,
    /// 没有注意保护关系。
    MissedProtection,
    /// 没有算清吃子后的回吃。
    MissedRecapture,
    /// 没有注意线路与炮架。
    MissedLines,
    /// 没有考虑对方的回应。
    MissedReply,
}

fn labels(board: &Board, squares: impl Iterator<Item = Square>) -> String {
    join_labels(&squares.map(|sq| label_at(board, sq)).collect::<Vec<_>>())
}

/// 这一着（卡片中棋谱的着法）涉及的主题。局面无法读取或着法不合规则时为空。
pub fn card_themes(fen_before: &str, mv: Move) -> Vec<ThemeTag> {
    let Some(f) = fen::parse_position(fen_before).ok().and_then(|pos| facts::move_facts(&pos, mv)) else { return Vec::new() };
    let (before, after) = (&f.before.board, &f.after.board);
    let mut out = Vec::new();
    let mut add = |theme: Theme, why: String| out.push(ThemeTag { theme, why });
    if f.was_in_check {
        add(Theme::KingSafety, "走之前正被将军，这一步应将".into());
    } else if !f.checkers.is_empty() {
        add(Theme::KingSafety, "这一步将军".into());
    }
    let threatened: Vec<Square> = f.threats_before.iter().filter(|t| !t.defended).map(|t| t.square).collect();
    if !threatened.is_empty() {
        add(Theme::Threatened, format!("走之前{}受到攻击、缺少保护", labels(before, threatened.into_iter())));
    }
    if !f.new_protects.is_empty() {
        add(Theme::Protection, format!("这一步保护了{}", labels(after, f.new_protects.iter().map(|r| r.target))));
    } else if !f.lost_protects.is_empty() {
        add(Theme::Protection, format!("这一步之后{}失去了保护", labels(after, f.lost_protects.iter().map(|r| r.target))));
    }
    if !f.discovered.is_empty() {
        add(Theme::Lines, format!("走动后打开了线路：{}可以攻击新的目标", labels(after, f.discovered.iter().map(|l| l.piece_square))));
    } else if !f.enemy_gains.is_empty() {
        add(Theme::Lines, format!("走动后对方{}的线路打开", labels(after, f.enemy_gains.iter().map(|l| l.piece_square))));
    }
    if let Some(p) = f.captured {
        let back = if facts::is_attacked_legally(after, f.mv.to) { "，对方可以吃回" } else { "，对方不能直接吃回" };
        add(Theme::Recapture, format!("这一步吃掉对方的{}{back}", p.name()));
    }
    out
}

#[cfg(test)]
mod tests {
    use xq_core::fen::START_FEN;

    use super::*;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    #[test]
    fn themes_come_from_rule_facts_with_their_reason() {
        let themes = |fen: &str, m: &str| card_themes(fen, mv(m)).into_iter().map(|t| (t.theme, t.why)).collect::<Vec<_>>();
        // 马二进三：保护了仕和中兵
        let horse = themes(START_FEN, "h0g2");
        assert_eq!(horse, vec![(Theme::Protection, "这一步保护了仕和中兵".to_string())]);
        // 炮二进七打马：吃子，对方的车可以吃回
        let capture = themes(START_FEN, "h2h9");
        assert!(capture.iter().any(|(t, why)| *t == Theme::Recapture && why == "这一步吃掉对方的马，对方可以吃回"), "{capture:?}");
        // 受到车攻击、没有保护的炮
        let attacked = themes("r3k4/9/9/9/C8/9/9/9/9/3K5 w", "a5b5");
        assert!(attacked.iter().any(|(t, why)| *t == Theme::Threatened && why.contains("受到攻击")), "{attacked:?}");
        // 无法读取的局面不给主题
        assert!(card_themes("not a fen", mv("h0g2")).is_empty());
    }
}
