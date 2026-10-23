//! 棋盘事实：完全由规则计算，不依赖引擎。只陈述关系（吃子、将军、攻击、保护、
//! 线路变化），是否因此“更好”由变化与候选比较另行判断。

use std::collections::HashSet;

use serde::Serialize;

use crate::board::Board;
use crate::board::Position;
use crate::notation;
use crate::rules;
use crate::rules::GameStatus;
use crate::types::Color;
use crate::types::Move;
use crate::types::Piece;
use crate::types::PieceKind;
use crate::types::Square;

/// 受到攻击且没有保护，或攻击者价值更低的棋子。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Threat {
    pub square: Square,
    pub piece: Piece,
    pub attackers: Vec<Square>,
    pub defended: bool,
}

fn value(board: &Board, sq: Square) -> i32 { board.get(sq).map(|p| p.value_at(sq)).unwrap_or(0) }

pub fn defenders(board: &Board, sq: Square) -> Vec<Square> {
    match board.get(sq) {
        Some(p) => rules::attackers(board, sq, p.color),
        None => Vec::new(),
    }
}

pub fn is_defended(board: &Board, sq: Square) -> bool { !defenders(board, sq).is_empty() }

/// 某方处于受攻击状态的棋子，按价值从高到低。
pub fn en_prise(board: &Board, color: Color) -> Vec<Threat> {
    let mut out = Vec::new();
    for (sq, piece) in board.pieces() {
        if piece.color != color || piece.kind == PieceKind::King {
            continue;
        }
        let defended = is_defended(board, sq);
        let mut attackers = rules::attackers(board, sq, color.opposite());
        // 将帅只能吃没有保护的棋子
        attackers.retain(|a| board.get(*a).is_some_and(|x| x.kind != PieceKind::King || !defended));
        let Some(cheapest) = attackers.iter().map(|a| value(board, *a)).min() else { continue };
        if !defended || cheapest < piece.value_at(sq) {
            out.push(Threat { square: sq, piece, attackers, defended });
        }
    }
    out.sort_by_key(|t| -t.piece.value_at(t.square));
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckResolution {
    KingMove,
    Capture,
    Block,
}

/// 走动的棋子与另一个棋子之间新建立或失去的关系。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Relation {
    pub target: Square,
    pub target_piece: Piece,
    /// 关系建立后（或失去后）目标是否仍有保护。
    pub defended: bool,
    /// 目标是否受到对方攻击。
    pub attacked: bool,
    /// 走子前目标是否已有保护。
    pub defended_before: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineVia {
    /// 走动的棋子让开了线路。
    Opened,
    /// 走动的棋子成为炮架。
    Screen,
}

/// 未走动的棋子因本步而新获得的攻击。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LineChange {
    pub piece_square: Square,
    pub piece: Piece,
    pub target: Square,
    pub target_piece: Piece,
    pub via: LineVia,
    pub target_defended: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Development {
    LeavesHome,
    CrossesRiver,
    CenterCannon,
    Defensive,
    KingMove,
}

#[derive(Clone, Debug)]
pub struct MoveFacts {
    pub mover: Color,
    pub piece: Piece,
    pub mv: Move,
    pub notation: String,
    pub before: Position,
    pub after: Position,
    pub captured: Option<Piece>,
    /// 走后将军对方的棋子（可能包含闪将的其他棋子）。
    pub checkers: Vec<Square>,
    pub opponent_status: GameStatus,
    pub was_in_check: bool,
    pub check_resolution: Option<CheckResolution>,
    pub threats_before: Vec<Threat>,
    pub threats_after: Vec<Threat>,
    pub new_attacks: Vec<Relation>,
    pub new_protects: Vec<Relation>,
    pub lost_protects: Vec<Relation>,
    pub discovered: Vec<LineChange>,
    pub enemy_gains: Vec<LineChange>,
    pub development: Option<Development>,
    pub mobility_before: usize,
    pub mobility_after: usize,
}

fn strictly_between(a: Square, b: Square, x: Square) -> bool {
    if a.file() == b.file() && x.file() == a.file() {
        let (lo, hi) = (a.rank().min(b.rank()), a.rank().max(b.rank()));
        x.rank() > lo && x.rank() < hi
    } else if a.rank() == b.rank() && x.rank() == a.rank() {
        let (lo, hi) = (a.file().min(b.file()), a.file().max(b.file()));
        x.file() > lo && x.file() < hi
    } else {
        false
    }
}

/// `owner` 方未走动的棋子因本步新获得的、针对对方非将帅棋子的攻击。
fn line_changes(before: &Board, after: &Board, owner: Color, mv: Move) -> Vec<LineChange> {
    let mut out = Vec::new();
    for (sq, piece) in after.pieces() {
        if piece.color != owner || sq == mv.to || before.get(sq) != Some(piece) {
            continue;
        }
        let old: HashSet<Square> = rules::controls(before, sq).into_iter().collect();
        for target in rules::controls(after, sq) {
            if old.contains(&target) {
                continue;
            }
            let Some(target_piece) = after.get(target) else { continue };
            if target_piece.color == owner || target_piece.kind == PieceKind::King {
                continue;
            }
            let via = if piece.kind == PieceKind::Cannon && strictly_between(sq, target, mv.to) {
                LineVia::Screen
            } else {
                LineVia::Opened
            };
            out.push(LineChange {
                piece_square: sq,
                piece,
                target,
                target_piece,
                via,
                target_defended: is_defended(after, target),
            });
        }
    }
    out.sort_by_key(|c| -c.target_piece.value_at(c.target));
    out
}

pub fn move_facts(pos: &Position, mv: Move) -> Option<MoveFacts> {
    let piece = pos.board.get(mv.from)?;
    let mover = piece.color;
    let enemy = mover.opposite();
    let before = &pos.board;
    let after_pos = pos.after(mv);
    let after = &after_pos.board;

    let was_in_check = rules::in_check(before, mover);
    let checkers_before = rules::checkers(before, mover);
    let captured = before.get(mv.to);
    let check_resolution = was_in_check.then(|| {
        if piece.kind == PieceKind::King {
            CheckResolution::KingMove
        } else if checkers_before.contains(&mv.to) {
            CheckResolution::Capture
        } else {
            CheckResolution::Block
        }
    });

    let ctrl_before: HashSet<Square> = rules::controls(before, mv.from).into_iter().collect();
    let ctrl_after = rules::controls(after, mv.to);

    let mut new_attacks = Vec::new();
    let mut new_protects = Vec::new();
    for target in &ctrl_after {
        let Some(target_piece) = after.get(*target) else { continue };
        if target_piece.kind == PieceKind::King || ctrl_before.contains(target) {
            continue;
        }
        let relation = Relation {
            target: *target,
            target_piece,
            defended: is_defended(after, *target),
            attacked: rules::is_attacked(after, *target, target_piece.color.opposite()),
            defended_before: before.get(*target) == Some(target_piece) && is_defended(before, *target),
        };
        if target_piece.color == enemy {
            new_attacks.push(relation);
        } else {
            new_protects.push(relation);
        }
    }

    let mut lost_protects = Vec::new();
    for target in &ctrl_before {
        let Some(target_piece) = before.get(*target) else { continue };
        if target_piece.color != mover
            || target_piece.kind == PieceKind::King
            || ctrl_after.contains(target)
            || after.get(*target) != Some(target_piece)
        {
            continue;
        }
        let defended = is_defended(after, *target);
        if !defended {
            lost_protects.push(Relation {
                target: *target,
                target_piece,
                defended,
                attacked: rules::is_attacked(after, *target, enemy),
                defended_before: true,
            });
        }
    }

    let development = match piece.kind {
        PieceKind::Horse | PieceKind::Rook if mv.from.rank() == mover.home_rank() => Some(Development::LeavesHome),
        PieceKind::Pawn if mover.own_half(mv.from.rank()) && !mover.own_half(mv.to.rank()) => {
            Some(Development::CrossesRiver)
        }
        PieceKind::Cannon if mv.to.file() == 4 && mv.from.file() != 4 => Some(Development::CenterCannon),
        PieceKind::Advisor | PieceKind::Elephant if mv.from.rank() == mover.home_rank() => Some(Development::Defensive),
        PieceKind::King => Some(Development::KingMove),
        _ => None,
    };

    Some(MoveFacts {
        mover,
        piece,
        mv,
        notation: notation::move_to_chinese(pos, mv).unwrap_or_else(|_| mv.iccs()),
        captured,
        checkers: rules::checkers(after, enemy),
        opponent_status: rules::game_status(&after_pos),
        was_in_check,
        check_resolution,
        threats_before: en_prise(before, mover),
        threats_after: en_prise(after, mover),
        new_attacks,
        new_protects,
        lost_protects,
        discovered: line_changes(before, after, mover, mv),
        enemy_gains: line_changes(before, after, enemy, mv),
        development,
        mobility_before: rules::pseudo_targets(before, mv.from).len(),
        mobility_after: rules::pseudo_targets(after, mv.to).len(),
        after: after_pos,
        before: pos.clone(),
    })
}

/// 局面层面的事实，用于根局面和任意节点的说明。
#[derive(Clone, Debug, Serialize)]
pub struct PositionFacts {
    pub side: Color,
    pub in_check: bool,
    pub checkers: Vec<Square>,
    pub status: GameStatus,
    pub legal_moves: usize,
    pub material_red: i32,
    pub material_black: i32,
    /// 行棋方受攻击的棋子。
    pub own_threats: Vec<Threat>,
    /// 对方受攻击的棋子（行棋方的机会）。
    pub opponent_threats: Vec<Threat>,
}

pub fn material(board: &Board, color: Color) -> i32 {
    board.pieces().filter(|(_, p)| p.color == color && p.kind != PieceKind::King).map(|(sq, p)| p.value_at(sq)).sum()
}

/// 某方子力清单，如“车2 马1 炮2 兵3 仕2 相2”。
pub fn material_inventory(board: &Board, color: Color) -> String {
    let order = [
        PieceKind::Rook,
        PieceKind::Horse,
        PieceKind::Cannon,
        PieceKind::Pawn,
        PieceKind::Advisor,
        PieceKind::Elephant,
    ];
    let parts: Vec<String> = order
        .iter()
        .filter_map(|kind| {
            let piece = Piece::new(color, *kind);
            let n = board.count(piece);
            (n > 0).then(|| format!("{}{}", piece.name(), n))
        })
        .collect();
    if parts.is_empty() { "只剩将帅".to_string() } else { parts.join(" ") }
}

pub fn position_facts(pos: &Position) -> PositionFacts {
    PositionFacts {
        side: pos.side,
        in_check: rules::in_check(&pos.board, pos.side),
        checkers: rules::checkers(&pos.board, pos.side),
        status: rules::game_status(pos),
        legal_moves: rules::legal_moves(pos).len(),
        material_red: material(&pos.board, Color::Red),
        material_black: material(&pos.board, Color::Black),
        own_threats: en_prise(&pos.board, pos.side),
        opponent_threats: en_prise(&pos.board, pos.side.opposite()),
    }
}

/// 沿一条变化计算行棋方子力差的变化（正数为行棋方得利）。遇到非法着法时停止。
pub fn material_swing(pos: &Position, line: &[Move], plies: usize) -> (i32, usize) {
    let me = pos.side;
    let start = material(&pos.board, me) - material(&pos.board, me.opposite());
    let mut cur = pos.clone();
    let mut played = 0;
    for mv in line.iter().take(plies) {
        if !rules::is_legal(&cur, *mv) {
            break;
        }
        cur = cur.after(*mv);
        played += 1;
    }
    let end = material(&cur.board, me) - material(&cur.board, me.opposite());
    (end - start, played)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_position;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    fn sq(s: &str) -> Square { Square::parse(s).unwrap() }

    #[test]
    fn horse_development_protects_center_pawn() {
        let f = move_facts(&Position::startpos(), mv("h0g2")).unwrap();
        assert_eq!(f.notation, "马二进三");
        assert_eq!(f.development, Some(Development::LeavesHome));
        let protected: Vec<Square> = f.new_protects.iter().map(|r| r.target).collect();
        assert!(protected.contains(&sq("e3")), "{protected:?}");
        let center = f.new_protects.iter().find(|r| r.target == sq("e3")).unwrap();
        assert!(!center.defended_before);
        assert!(f.captured.is_none() && f.checkers.is_empty());
        // 马二进一不形成新的保护
        let alt = move_facts(&Position::startpos(), mv("h0i2")).unwrap();
        assert!(alt.new_protects.iter().all(|r| r.target != sq("e3")));
    }

    #[test]
    fn capture_and_recapture_risk() {
        // 炮二平五、马8进7 后，炮五进四打中卒：落点会被马吃
        let pos = parse_position("rnbakab1r/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR w").unwrap();
        let f = move_facts(&pos, mv("e2e6")).unwrap();
        assert_eq!(f.captured.map(|p| p.kind), Some(PieceKind::Pawn));
        assert!(f.threats_after.iter().any(|t| t.square == sq("e6")), "{:?}", f.threats_after);
    }

    #[test]
    fn check_and_escape() {
        // 车平六将军，黑将只能去 e9 却与帅照面：将死
        let pos = parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 w").unwrap();
        let f = move_facts(&pos, mv("e1d1")).unwrap();
        assert_eq!(f.checkers, vec![sq("d1")]);
        assert_eq!(f.opponent_status, GameStatus::Checkmate);
        let pos = parse_position("3k5/9/9/9/9/9/9/9/3R5/5K3 b").unwrap();
        let g = move_facts(&pos, mv("d9e9")).unwrap();
        assert!(g.was_in_check);
        assert_eq!(g.check_resolution, Some(CheckResolution::KingMove));
    }

    #[test]
    fn cannon_screen_is_detected() {
        // 红马跳到炮与黑车之间，成为炮架
        let pos = parse_position("4k4/9/9/4r4/9/9/9/2N6/4C4/3K5 w").unwrap();
        let f = move_facts(&pos, mv("c2e3")).unwrap();
        assert!(f.discovered.iter().any(|c| c.via == LineVia::Screen && c.target == sq("e6")), "{:?}", f.discovered);
    }

    #[test]
    fn swing_along_line() {
        let pos = parse_position("rnbakab1r/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR w").unwrap();
        let (swing, played) = material_swing(&pos, &[mv("e2e6"), mv("g7e6")], 4);
        assert_eq!(played, 2);
        assert!(swing < -300, "{swing}");
    }
}
