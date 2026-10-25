//! 先想后讲的分级提示。提示只依据落子前局面的事实与该局面的候选生成，不引用棋谱中的实际着法。
//!
//! 一级：引导观察局面需求，不点名具体棋子；
//! 二级：指出与这项需求相关的棋子（界面在棋盘上标出）；
//! 三级：给出几步可以考虑的着法，按棋盘位置排列，不带评分与名次，由用户继续选择。
//!
//! 局面需求按顺序选取第一项成立的：正被将军、己方有受攻击的棋子、对方有能被攻击到的弱点、
//! 车马仍在底线、其他情况。

use serde::Deserialize;
use serde::Serialize;

use super::compose::EngineEvidence;
use super::compose::line_is_legal;
use super::describe::join_labels;
use super::describe::label_at;
use super::facts;
use crate::Move;
use crate::PieceKind;
use crate::Position;
use crate::Square;
use crate::notation;
use crate::rules;
use crate::types::Color;

/// 局面需求的类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintTopic {
    InCheck,
    OwnThreat,
    Opportunity,
    Development,
    Improve,
}

/// 二级提示：相关的棋子与关系。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HintFocus {
    pub text: String,
    pub squares: Vec<Square>,
    pub arrows: Vec<Move>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HintMove {
    pub mv: Move,
    pub notation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkHints {
    pub side: Color,
    pub topic: HintTopic,
    /// 一级：观察局面需求。
    pub observe: String,
    /// 二级：指出相关的棋子。
    pub focus: HintFocus,
    /// 三级：可以考虑的着法（不分先后）。
    pub moves: Vec<HintMove>,
    /// 三级提示的来源说明。
    pub moves_note: String,
}

/// 练习时实际看过的提示：只保留用到的级数，供复习时回看当时的思考。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsedHints {
    pub observe: String,
    /// 用到二级时才有。
    pub focus: Option<HintFocus>,
    /// 用到三级时才有。
    pub moves: Vec<HintMove>,
}

impl ThinkHints {
    /// 前 `level` 级提示；没有用提示时为 `None`。
    pub fn used(&self, level: u8) -> Option<UsedHints> {
        UsedHints { observe: self.observe.clone(), focus: Some(self.focus.clone()), moves: self.moves.clone() }.up_to(level)
    }
}

impl UsedHints {
    /// 只保留前 `level` 级；没有用提示时为 `None`。
    pub fn up_to(mut self, level: u8) -> Option<UsedHints> {
        if level == 0 {
            return None;
        }
        if level < 2 {
            self.focus = None;
        }
        if level < 3 {
            self.moves.clear();
        }
        Some(self)
    }
}

const MAX_MOVES: usize = 3;

/// 生成某个局面的分级提示；局面已经结束（无着可走）时返回 `None`。
pub fn think_hints(pos: &Position, engine: Option<&EngineEvidence>) -> Option<ThinkHints> {
    let legal = rules::legal_moves(pos);
    if legal.is_empty() {
        return None;
    }
    let f = facts::position_facts(pos);
    let board = &pos.board;
    let side = pos.side;
    let label_list = |squares: &[Square]| join_labels(&squares.iter().map(|sq| label_at(board, *sq)).collect::<Vec<_>>());

    let own: Vec<&facts::Threat> = f.own_threats.iter().filter(|t| !t.defended).take(2).collect();
    let chances: Vec<&facts::Threat> = f.opponent_threats.iter().filter(|t| !t.defended).take(2).collect();
    let home: Vec<Square> = board
        .pieces()
        .filter(|(sq, p)| p.color == side && matches!(p.kind, PieceKind::Rook | PieceKind::Horse) && sq.rank() == side.home_rank())
        .map(|(sq, _)| sq)
        .collect();

    // 选取局面需求，并给出规则上与之相关的着法（没有引擎候选时作为三级提示）
    let (topic, observe, focus, related): (HintTopic, &str, HintFocus, Vec<Move>) = if f.in_check {
        let king = board.find_king(side);
        let focus = HintFocus {
            text: format!("正在将军的是{}", label_list(&f.checkers)),
            squares: f.checkers.iter().copied().chain(king).collect(),
            arrows: king.map(|k| f.checkers.iter().map(|c| Move::new(*c, k)).collect()).unwrap_or_default(),
        };
        (HintTopic::InCheck, "你正被将军，先想怎样应将：走开将帅、吃掉将军的棋子，或者挡住将军的路线。", focus, legal.clone())
    } else if !own.is_empty() {
        let text = own
            .iter()
            .map(|t| format!("{}受到{}攻击", label_at(board, t.square), label_list(&t.attackers)))
            .collect::<Vec<_>>()
            .join("；");
        let focus = HintFocus {
            text,
            squares: own.iter().flat_map(|t| std::iter::once(t.square).chain(t.attackers.iter().copied())).collect(),
            arrows: own.iter().flat_map(|t| t.attackers.iter().map(move |a| Move::new(*a, t.square))).collect(),
        };
        // 走开受攻击的棋子，或吃掉攻击者
        let related = legal
            .iter()
            .copied()
            .filter(|m| own.iter().any(|t| m.from == t.square || t.attackers.contains(&m.to)))
            .collect();
        (HintTopic::OwnThreat, "先检查己方是否有受到直接威胁、又缺少保护的棋子。", focus, related)
    } else if !chances.is_empty() {
        let text = chances
            .iter()
            .map(|t| format!("对方{}可以被{}吃到", label_at(board, t.square), label_list(&t.attackers)))
            .collect::<Vec<_>>()
            .join("；");
        let focus = HintFocus {
            text,
            squares: chances.iter().flat_map(|t| std::iter::once(t.square).chain(t.attackers.iter().copied())).collect(),
            arrows: chances.iter().flat_map(|t| t.attackers.iter().map(move |a| Move::new(*a, t.square))).collect(),
        };
        let related = legal.iter().copied().filter(|m| chances.iter().any(|t| m.to == t.square)).collect();
        (HintTopic::Opportunity, "看看对方有没有缺少保护、又能被攻击到的棋子。", focus, related)
    } else if home.len() >= 2 {
        let focus = HintFocus { text: format!("{}仍在底线，尚未出动", label_list(&home)), squares: home.clone(), arrows: vec![] };
        let related = legal.iter().copied().filter(|m| home.contains(&m.from)).collect();
        (HintTopic::Development, "当前没有直接威胁，想想还没有出动的子力。", focus, related)
    } else {
        let focus = HintFocus { text: "这一步没有受攻击的棋子，也没有明显的吃子机会".to_string(), squares: vec![], arrows: vec![] };
        (HintTopic::Improve, "没有直接的威胁或机会，想想哪个棋子的位置最需要改善，或者能对对方施加压力。", focus, vec![])
    };

    // 三级：优先使用该局面的引擎候选（能合法回放的首着），否则使用与需求相关的着法
    let engine_moves: Vec<Move> = engine
        .map(|e| {
            e.candidates
                .iter()
                .filter(|l| legal.contains(&l.mv) && line_is_legal(pos, &l.pv))
                .map(|l| l.mv)
                .take(MAX_MOVES)
                .collect()
        })
        .unwrap_or_default();
    let (mut picked, moves_note) = if engine_moves.len() >= 2 {
        (engine_moves, "引擎候选中的几步，按棋盘位置排列，不分先后".to_string())
    } else if !related.is_empty() {
        let mut r = related;
        r.sort_by_key(|m| board_order(*m, side));
        r.dedup();
        r.truncate(MAX_MOVES);
        (r, "按局面需求挑选的几步（未经引擎检验），按棋盘位置排列".to_string())
    } else {
        (Vec::new(), "还没有可以参考的候选，可以先比较每个棋子的去处".to_string())
    };
    // 不按名次排列，避免透露排序
    picked.sort_by_key(|m| board_order(*m, side));
    let moves = picked
        .into_iter()
        .map(|mv| HintMove { mv, notation: notation::move_to_chinese(pos, mv).unwrap_or_else(|_| mv.iccs()) })
        .collect();
    Some(ThinkHints { side, topic, observe: observe.to_string(), focus, moves, moves_note })
}

/// 按走棋方看到的棋盘位置排列（从右到左、从近到远），与搜索名次无关。
fn board_order(m: Move, side: Color) -> (u8, u8, u8, u8) {
    let (f, r) = (m.from.file(), m.from.rank());
    match side {
        Color::Red => (8 - f, r, m.to.file(), m.to.rank()),
        Color::Black => (f, 9 - r, m.to.file(), m.to.rank()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explain::compose::EngineLine;
    use crate::fen::parse_position;
    use crate::uci::Bound;
    use crate::uci::Score;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    fn line(m: &str, cp: i32) -> EngineLine {
        EngineLine { mv: mv(m), score: Score::Cp(cp), bound: Bound::Exact, depth: 12, pv: vec![mv(m)] }
    }

    fn engine(lines: Vec<EngineLine>) -> EngineEvidence {
        EngineEvidence { candidates: lines, actual: None, conditions: "测试".into(), complete: true, notes: vec![] }
    }

    #[test]
    fn opening_without_threats_points_to_development() {
        let pos = parse_position("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w").unwrap();
        let h = think_hints(&pos, None).unwrap();
        assert_eq!(h.topic, HintTopic::Development);
        assert!(h.observe.contains("没有出动的子力"));
        assert!(h.focus.text.contains("仍在底线") && h.focus.squares.len() >= 2);
        assert!(!h.moves.is_empty() && h.moves.iter().all(|m| h.focus.squares.contains(&m.mv.from)), "{:?}", h.moves);
        assert!(h.moves_note.contains("未经引擎检验"));
    }

    #[test]
    fn check_comes_first_and_marks_checker_and_king() {
        // 红车将军黑将
        let pos = parse_position("4k4/9/9/9/9/9/9/9/4R4/3K5 b").unwrap();
        let h = think_hints(&pos, None).unwrap();
        assert_eq!(h.topic, HintTopic::InCheck);
        assert!(h.focus.text.contains("车"), "{}", h.focus.text);
        assert!(h.focus.squares.contains(&Square::parse("e1").unwrap()) && h.focus.squares.contains(&Square::parse("e9").unwrap()));
        assert_eq!(h.focus.arrows, vec![mv("e1e9")]);
    }

    #[test]
    fn own_undefended_piece_under_attack_is_named_only_at_level_two() {
        // 红中炮打黑中卒，中卒没有保护
        let pos = parse_position("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKAB1R b").unwrap();
        let h = think_hints(&pos, None).unwrap();
        assert_eq!(h.topic, HintTopic::OwnThreat);
        assert!(!h.observe.contains("卒"), "一级提示不点名具体棋子：{}", h.observe);
        assert!(h.focus.text.contains("中卒受到中炮攻击"), "{}", h.focus.text);
        assert!(h.moves.iter().all(|m| m.mv.from == Square::parse("e6").unwrap() || m.mv.to == Square::parse("e2").unwrap()), "{:?}", h.moves);
    }

    #[test]
    fn engine_candidates_are_listed_without_rank_order() {
        let pos = parse_position("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w").unwrap();
        // 名次：炮二平五、兵七进一、马八进七；提示按棋盘位置（从右到左）排列
        let h = think_hints(&pos, Some(&engine(vec![line("h2e2", 40), line("c3c4", 30), line("b0c2", 20)]))).unwrap();
        let names: Vec<&str> = h.moves.iter().map(|m| m.notation.as_str()).collect();
        assert_eq!(names, vec!["炮二平五", "兵七进一", "马八进七"]);
        let h2 = think_hints(&pos, Some(&engine(vec![line("b0c2", 40), line("h2e2", 30), line("c3c4", 20)]))).unwrap();
        assert_eq!(h2.moves, h.moves, "名次改变不影响提示的顺序");
        assert!(h.moves_note.contains("不分先后"));
    }

    #[test]
    fn finished_position_has_no_hints() {
        // 黑方被将死
        let pos = parse_position("3k5/3R5/3R5/9/9/9/9/9/9/4K4 b").unwrap();
        assert!(think_hints(&pos, None).is_none());
    }
}
