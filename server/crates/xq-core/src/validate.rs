//! 局面校验：格式错误由 `fen` 报告，这里负责静态局面（数量、位置、将帅照面）
//! 与行棋方相关的检查（非行棋方被将军、终局状态）。

use serde::Serialize;

use crate::board::Board;
use crate::board::Position;
use crate::rules;
use crate::rules::GameStatus;
use crate::types::Color;
use crate::types::Piece;
use crate::types::PieceKind;
use crate::types::Square;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub squares: Vec<Square>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    /// 没有错误级问题，可以开始分析。
    pub ok: bool,
    pub issues: Vec<Issue>,
    /// 仅在静态校验通过且给出行棋方时计算。
    pub status: Option<GameStatus>,
    pub in_check: bool,
}

fn error(code: &'static str, message: String, squares: Vec<Square>) -> Issue {
    Issue { code, severity: Severity::Error, message, squares }
}

fn advisor_points(color: Color) -> [(u8, u8); 5] {
    let ranks = match color {
        Color::Red => (0, 1, 2),
        Color::Black => (9, 8, 7),
    };
    [(3, ranks.0), (5, ranks.0), (4, ranks.1), (3, ranks.2), (5, ranks.2)]
}

fn elephant_points(color: Color) -> [(u8, u8); 7] {
    let (r0, r2, r4) = match color {
        Color::Red => (0, 2, 4),
        Color::Black => (9, 7, 5),
    };
    [(2, r0), (6, r0), (0, r2), (4, r2), (8, r2), (2, r4), (6, r4)]
}

fn placement_ok(piece: Piece, sq: Square) -> bool {
    let at = (sq.file(), sq.rank());
    match piece.kind {
        PieceKind::King => piece.color.in_palace(sq),
        PieceKind::Advisor => advisor_points(piece.color).contains(&at),
        PieceKind::Elephant => elephant_points(piece.color).contains(&at),
        PieceKind::Pawn => {
            // 兵卒不能在己方兵线之后；未过河时只能在兵线所在的五条纵线上
            let (behind, before_river) = match piece.color {
                Color::Red => (sq.rank() < 3, sq.rank() <= 4),
                Color::Black => (sq.rank() > 6, sq.rank() >= 5),
            };
            !behind && !(before_river && sq.file() % 2 == 1)
        }
        _ => true,
    }
}

fn placement_message(piece: Piece, sq: Square) -> String {
    let name = piece.name();
    let side = piece.color.label();
    match piece.kind {
        PieceKind::King => format!("{side}{name}不在九宫内（{sq}）"),
        PieceKind::Advisor => format!("{side}{name}只能位于九宫的五个斜线点上（{sq}）"),
        PieceKind::Elephant => format!("{side}{name}只能位于本方七个象位上（{sq}）"),
        PieceKind::Pawn => format!("{side}{name}不可能出现在 {sq}：兵卒不能后退，过河前也不能横走"),
        _ => format!("{side}{name}位置异常（{sq}）"),
    }
}

/// 静态局面检查：与行棋方无关。
pub fn validate_board(board: &Board) -> Vec<Issue> {
    let mut issues = Vec::new();
    for color in [Color::Red, Color::Black] {
        for kind in PieceKind::ALL {
            let piece = Piece::new(color, kind);
            let squares: Vec<Square> = board.pieces().filter(|(_, p)| *p == piece).map(|(sq, _)| sq).collect();
            if kind == PieceKind::King && squares.is_empty() {
                issues.push(error("missing_king", format!("缺少{}的{}", color.label(), piece.name()), vec![]));
            }
            if squares.len() > kind.max_count() {
                issues.push(error(
                    "too_many",
                    format!("{}{}共有 {} 个，最多 {} 个", color.label(), piece.name(), squares.len(), kind.max_count()),
                    squares.clone(),
                ));
            }
            for sq in squares {
                if !placement_ok(piece, sq) {
                    issues.push(error("placement", placement_message(piece, sq), vec![sq]));
                }
            }
        }
    }
    if rules::kings_facing(board) {
        let squares = [Color::Red, Color::Black].iter().filter_map(|c| board.find_king(*c)).collect();
        issues.push(error("kings_facing", "将帅在同一纵线上直接照面，局面不成立".to_string(), squares));
    }
    issues
}

/// 完整局面检查：静态检查 + 行棋方相关检查 + 终局状态。
pub fn validate_position(pos: &Position) -> ValidationReport {
    let mut issues = validate_board(&pos.board);
    let static_ok = issues.iter().all(|i| i.severity != Severity::Error);
    let waiting = pos.side.opposite();
    if static_ok && rules::in_check(&pos.board, waiting) {
        let mut squares = rules::checkers(&pos.board, waiting);
        squares.extend(pos.board.find_king(waiting));
        issues.push(error(
            "waiting_side_in_check",
            format!(
                "轮到{}走，但{}正被将军：实际对局中{}上一步必须先应将，请检查行棋方或棋子位置",
                pos.side.label(),
                waiting.label(),
                waiting.label()
            ),
            squares,
        ));
    }
    let ok = issues.iter().all(|i| i.severity != Severity::Error);
    let in_check = ok && rules::in_check(&pos.board, pos.side);
    let status = ok.then(|| rules::game_status(pos));
    if let Some(s) = status {
        let message = match s {
            GameStatus::Checkmate => Some(format!("{}已被将死", pos.side.label())),
            GameStatus::Stalemate => Some(format!("{}没有合法着法（困毙），按规则判负", pos.side.label())),
            GameStatus::Ongoing => None,
        };
        if let Some(message) = message {
            issues.push(Issue { code: "terminal", severity: Severity::Warning, message, squares: vec![] });
        }
    }
    ValidationReport { ok, issues, status, in_check }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_fen;
    use crate::fen::parse_position;

    fn codes(report: &ValidationReport) -> Vec<&'static str> { report.issues.iter().map(|i| i.code).collect() }

    #[test]
    fn startpos_is_valid() {
        let r = validate_position(&Position::startpos());
        assert!(r.ok, "{:?}", r.issues);
        assert_eq!(r.status, Some(GameStatus::Ongoing));
    }

    #[test]
    fn detects_static_problems() {
        let board = parse_fen("4k4/9/9/9/9/9/9/9/9/9").unwrap().board;
        assert!(validate_board(&board).iter().any(|i| i.code == "missing_king"));
        let board = parse_fen("4k4/9/9/9/9/9/9/9/9/4K4").unwrap().board;
        assert!(validate_board(&board).iter().any(|i| i.code == "kings_facing"));
        // 红兵在本方兵线后、红相在非象位
        let board = parse_fen("3k5/9/9/9/9/9/9/9/4P4/3KB4").unwrap().board;
        let issues = validate_board(&board);
        assert_eq!(issues.iter().filter(|i| i.code == "placement").count(), 2);
        let board = parse_fen("3k5/9/9/9/9/9/RRR6/9/9/4K4").unwrap().board;
        assert!(validate_board(&board).iter().any(|i| i.code == "too_many"));
    }

    #[test]
    fn side_dependent_checks() {
        // 黑将被车将军，却轮到红走
        let pos = parse_position("3k5/3R5/9/9/9/9/9/9/9/4K4 w").unwrap();
        let r = validate_position(&pos);
        assert!(!r.ok);
        assert!(codes(&r).contains(&"waiting_side_in_check"));
        // 同一局面轮到黑走则成立，且黑方正被将军
        let pos = parse_position("3k5/3R5/9/9/9/9/9/9/9/4K4 b").unwrap();
        let r = validate_position(&pos);
        assert!(r.ok);
        assert!(r.in_check);
    }

    #[test]
    fn terminal_is_reported_not_rejected() {
        let r = validate_position(&parse_position("3k5/4R4/4R4/9/9/9/9/9/9/4K4 b").unwrap());
        assert!(r.ok);
        assert_eq!(r.status, Some(GameStatus::Stalemate));
        assert!(codes(&r).contains(&"terminal"));
    }
}
