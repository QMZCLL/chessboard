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

/// 受到威胁的棋子：对方吃下后己方无法吃回，或攻击者价值更低。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Threat {
    pub square: Square,
    pub piece: Piece,
    pub attackers: Vec<Square>,
    /// 吃下该子后己方无法合法吃回的攻击者。
    pub unanswered: Vec<Square>,
    /// 对方每一种吃法之后己方都能吃回（威胁只来自攻击者价值更低）。
    pub defended: bool,
}

fn value(board: &Board, sq: Square) -> i32 { board.get(sq).map(|p| p.value_at(sq)).unwrap_or(0) }

/// `by` 一方能合法走到 `sq` 吃子的棋子（排除被牵制、走后送将的棋子）。
pub fn legal_attackers(board: &Board, sq: Square, by: Color) -> Vec<Square> {
    let pos = Position::new(board.clone(), by);
    rules::attackers(board, sq, by).into_iter().filter(|from| rules::is_legal(&pos, Move::new(*from, sq))).collect()
}

/// 对方用 `attacker` 吃掉 `sq` 上的棋子后，原主能合法吃回的棋子。
///
/// 在棋盘上真正执行这次吃子：攻击者离开原位可能拆掉炮架或打开线路，回吃必须按吃子后的棋盘判断。
pub fn recapturers(board: &Board, attacker: Square, sq: Square) -> Vec<Square> {
    let Some(owner) = board.get(sq).map(|p| p.color) else { return Vec::new() };
    legal_attackers(&board.applied(Move::new(attacker, sq)), sq, owner)
}

/// 对方暂时吃不到 `sq` 时，假设对方棋子来到这里，`owner` 能否合法吃掉它。
fn covers(board: &Board, sq: Square, owner: Color) -> bool {
    let mut hypothetical = board.clone();
    hypothetical.set(sq, Some(Piece::new(owner.opposite(), PieceKind::Pawn)));
    !legal_attackers(&hypothetical, sq, owner).is_empty()
}

/// 该点上的棋子是否有真实可用的保护。
///
/// 对方能合法吃它时，逐一执行每一种吃法，每一种之后己方都能合法吃回才算有保护；
/// 对方暂时吃不到时，按假设对方棋子来到此处判断。
pub fn is_defended(board: &Board, sq: Square) -> bool {
    let Some(piece) = board.get(sq) else { return false };
    let attackers = legal_attackers(board, sq, piece.color.opposite());
    if attackers.is_empty() {
        covers(board, sq, piece.color)
    } else {
        attackers.iter().all(|a| !recapturers(board, *a, sq).is_empty())
    }
}

/// 对方能合法吃掉该点棋子。
pub fn is_attacked_legally(board: &Board, sq: Square) -> bool {
    board.get(sq).is_some_and(|p| !legal_attackers(board, sq, p.color.opposite()).is_empty())
}

/// `protector` 对 `target` 上己方棋子的保护：对方吃掉目标后，它能否合法吃回。
///
/// 目标受到攻击时逐一执行对方的吃法，返回它能应对的攻击者，一个都应对不了时为 `None`；
/// 目标暂未受到攻击时按假设判断，能保护时返回空列表。
fn protection(board: &Board, protector: Square, target: Square) -> Option<Vec<Square>> {
    let owner = board.get(target)?.color;
    let recaptures = |b: Board| rules::is_legal(&Position::new(b, owner), Move::new(protector, target));
    let attackers = legal_attackers(board, target, owner.opposite());
    if attackers.is_empty() {
        let mut probe = board.clone();
        probe.set(target, Some(Piece::new(owner.opposite(), PieceKind::Pawn)));
        return recaptures(probe).then(Vec::new);
    }
    let against: Vec<Square> =
        attackers.into_iter().filter(|a| recaptures(board.applied(Move::new(*a, target)))).collect();
    (!against.is_empty()).then_some(against)
}

/// 某方受到威胁的棋子，按价值从高到低。
///
/// 逐一执行对方每一种合法吃法：吃下后己方无法合法吃回，或攻击者价值更低（吃回也亏），都构成威胁。
pub fn en_prise(board: &Board, color: Color) -> Vec<Threat> {
    let mut out = Vec::new();
    for (sq, piece) in board.pieces() {
        if piece.color != color || piece.kind == PieceKind::King {
            continue;
        }
        let attackers = legal_attackers(board, sq, color.opposite());
        let unanswered: Vec<Square> =
            attackers.iter().copied().filter(|a| recapturers(board, *a, sq).is_empty()).collect();
        let cheaper = attackers.iter().any(|a| value(board, *a) < piece.value_at(sq));
        if !unanswered.is_empty() || cheaper {
            out.push(Threat { square: sq, piece, defended: unanswered.is_empty(), unanswered, attackers });
        }
    }
    out.sort_by_key(|t| -t.piece.value_at(t.square));
    out
}

/// 应将方式：按走子前后攻击关系为什么消失来区分。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckResolution {
    /// 移动将帅。
    KingMove,
    /// 吃掉正在将军的棋子。
    Capture,
    /// 挡在车（或兵以外直线攻击者）与将帅之间。
    BlockLine,
    /// 别住将军之马的马腿。
    BlockLeg,
    /// 移走炮与将帅之间唯一的炮架。
    RemoveScreen,
    /// 在炮与将帅之间增加棋子，炮隔两子无法吃将。
    AddScreen,
    /// 其他方式（理论上不应出现，保留以免误判）。
    Other,
}

/// 判断这一步为什么解除了将军。以第一个将军的棋子为准（双将只能动将或吃子化解）。
fn resolve_check(before: &Board, mv: Move, piece: Piece, checkers: &[Square]) -> CheckResolution {
    if piece.kind == PieceKind::King {
        return CheckResolution::KingMove;
    }
    if checkers.contains(&mv.to) {
        return CheckResolution::Capture;
    }
    let (Some(&checker_sq), Some(king)) = (checkers.first(), before.find_king(piece.color)) else {
        return CheckResolution::Other;
    };
    let Some(checker) = before.get(checker_sq) else { return CheckResolution::Other };
    let line = rules::squares_between(checker_sq, king);
    match checker.kind {
        PieceKind::Horse if rules::horse_leg(checker_sq, king) == Some(mv.to) => CheckResolution::BlockLeg,
        PieceKind::Cannon if line.contains(&mv.from) && !line.contains(&mv.to) => CheckResolution::RemoveScreen,
        PieceKind::Cannon if line.contains(&mv.to) => CheckResolution::AddScreen,
        PieceKind::Rook if line.contains(&mv.to) => CheckResolution::BlockLine,
        _ => CheckResolution::Other,
    }
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
    /// 保护关系对应的对方吃子棋子：它们吃掉目标后，走动的棋子可以合法吃回。
    /// 目标暂未受到攻击（按假设判断的保护）以及攻击关系时为空。
    pub against: Vec<Square>,
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
    /// 这枚棋子吃掉目标后，对方能否合法吃回。
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
            // 被牵制的棋子得到的线路并不能实际用来吃子
            if !rules::is_legal(&Position::new(after.clone(), owner), Move::new(sq, target)) {
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
                target_defended: !recapturers(after, sq, target).is_empty(),
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
    let check_resolution = was_in_check.then(|| resolve_check(before, mv, piece, &checkers_before));

    let ctrl_before: HashSet<Square> = rules::controls(before, mv.from).into_iter().collect();
    let ctrl_after = rules::controls(after, mv.to);

    let mut new_attacks = Vec::new();
    let mut new_protects = Vec::new();
    for target in &ctrl_after {
        let Some(target_piece) = after.get(*target) else { continue };
        if target_piece.kind == PieceKind::King || ctrl_before.contains(target) {
            continue;
        }
        let defended_before = before.get(*target) == Some(target_piece) && is_defended(before, *target);
        if target_piece.color == enemy {
            // 走到新位置的棋子若被牵制，它的攻击并不能实际兑现
            if !rules::is_legal(&Position::new(after.clone(), mover), Move::new(mv.to, *target)) {
                continue;
            }
            new_attacks.push(Relation {
                target: *target,
                target_piece,
                defended: !recapturers(after, mv.to, *target).is_empty(),
                attacked: true,
                defended_before,
                against: Vec::new(),
            });
        } else if let Some(against) = protection(after, mv.to, *target) {
            new_protects.push(Relation {
                target: *target,
                target_piece,
                defended: is_defended(after, *target),
                attacked: is_attacked_legally(after, *target),
                defended_before,
                against,
            });
        }
    }

    let mut lost_protects = Vec::new();
    for target in &ctrl_before {
        let Some(target_piece) = before.get(*target) else { continue };
        if target_piece.color != mover
            || target_piece.kind == PieceKind::King
            || ctrl_after.contains(target)
            || after.get(*target) != Some(target_piece)
            || protection(before, mv.from, *target).is_none()
        {
            continue;
        }
        let defended = is_defended(after, *target);
        if !defended {
            lost_protects.push(Relation {
                target: *target,
                target_piece,
                defended,
                attacked: is_attacked_legally(after, *target),
                defended_before: true,
                against: Vec::new(),
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
    fn check_resolution_mechanisms() {
        let cases = [
            // 评审中的用例：红马是黑炮与红帅之间唯一的炮架，跳开后炮不能再将军
            ("3k5/9/4c4/9/9/9/9/4N4/9/4K4 w", "e2c3", CheckResolution::RemoveScreen),
            // 车走到炮与帅之间，炮前变成两个棋子
            ("3k5/9/4c4/9/9/3R5/9/4N4/9/4K4 w", "d4e4", CheckResolution::AddScreen),
            // 车挡住黑车的将军线路
            ("3k5/4r4/9/9/9/9/R8/9/9/4K4 w", "a3e3", CheckResolution::BlockLine),
            // 车别住黑马的马腿
            ("3k5/9/9/9/9/9/9/3n5/R8/4K4 w", "a1d1", CheckResolution::BlockLeg),
            ("3k5/9/9/9/9/9/9/9/3R5/5K3 b", "d9e9", CheckResolution::KingMove),
        ];
        for (fen, m, expected) in cases {
            let pos = parse_position(fen).unwrap();
            assert!(rules::is_legal(&pos, mv(m)), "{fen} {m}");
            let f = move_facts(&pos, mv(m)).unwrap();
            assert!(f.was_in_check, "{fen}");
            assert_eq!(f.check_resolution, Some(expected), "{fen} {m}");
        }
    }

    #[test]
    fn pinned_piece_is_not_a_defender() {
        // 红马几何上保护 g3 兵，但被 e8 黑车牵制在帅前，黑车吃兵后马不能合法吃回
        let pos = parse_position("3k2r2/4r4/9/9/9/9/6P2/4N4/9/4K4 w").unwrap();
        assert!(rules::attackers(&pos.board, sq("g3"), Color::Red).contains(&sq("e2")));
        assert!(!is_defended(&pos.board, sq("g3")));
        let threats = en_prise(&pos.board, Color::Red);
        let pawn = threats.iter().find(|t| t.square == sq("g3")).expect("兵应被视为受攻击且无有效保护");
        assert!(!pawn.defended);
    }

    #[test]
    fn capturing_screen_is_not_a_protection() {
        // 评审中的用例：炮走到 a3 后，黑车 c3 看似成为炮架；但黑车吃马时离开 c3，炮无法吃回
        let pos = parse_position("3k5/9/9/9/9/C8/2r1N4/9/9/4K4 w").unwrap();
        let f = move_facts(&pos, mv("a4a3")).unwrap();
        assert!(f.new_protects.iter().all(|r| r.target != sq("e3")), "{:?}", f.new_protects);
        assert!(!is_defended(&f.after.board, sq("e3")));
        let horse = f.threats_after.iter().find(|t| t.square == sq("e3")).expect("马仍受到攻击");
        assert_eq!(horse.unanswered, vec![sq("c3")]);
    }

    #[test]
    fn protection_is_bound_to_the_capture() {
        // 车平到马所在横线：黑车吃马后，红车可以吃回
        let pos = parse_position("4rk3/9/9/9/4N4/R8/9/9/9/3K5 w").unwrap();
        let f = move_facts(&pos, mv("a4a5")).unwrap();
        let rel = f.new_protects.iter().find(|r| r.target == sq("e5")).expect("新增对马的保护");
        assert!(rel.attacked && rel.defended && !rel.defended_before);
        assert_eq!(rel.against, vec![sq("e9")]);
        assert!(f.threats_before.iter().any(|t| t.square == sq("e5")));
        assert!(f.threats_after.iter().all(|t| t.square != sq("e5")), "{:?}", f.threats_after);
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
