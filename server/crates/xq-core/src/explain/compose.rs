//! 把事实与引擎证据组织成讲解：一句话结论、当前需求、具体依据、候选比较、主要应对、学习点。

use serde::Deserialize;
use serde::Serialize;

use super::describe::join_labels;
use super::describe::label_at;
use super::describe::material_phrase;
use super::describe::piece_label;
use super::facts;
use super::facts::CheckResolution;
use super::facts::Development;
use super::facts::LineVia;
use super::facts::MoveFacts;
use crate::board::Position;
use crate::notation;
use crate::rules;
use crate::rules::GameStatus;
use crate::types::Color;
use crate::types::Move;
use crate::types::Piece;
use crate::types::PieceKind;
use crate::types::Square;
use crate::uci::Bound;
use crate::uci::Score;

/// 模板版本，随讲解一起保存，便于判断缓存是否过期。
pub const GENERATOR_VERSION: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Fact,
    Variation,
    Positional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    /// 由规则直接计算。
    Certain,
    /// 由引擎变化支持，受分析预算限制。
    Supported,
    /// 位置性判断，只作参考。
    Tentative,
}

/// 变化的起点：讲解某一着时为落子前局面，说明某个局面时为该局面本身。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineBase {
    Parent,
    Node,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRef {
    pub base: LineBase,
    pub moves: Vec<Move>,
    pub notation: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub kind: EvidenceKind,
    pub topic: String,
    pub text: String,
    pub squares: Vec<Square>,
    pub arrows: Vec<Move>,
    pub line: Option<LineRef>,
    pub certainty: Certainty,
    /// `squares` 与 `arrows` 所在的局面：讲解某一着时，落子前（如当前需求、应将、首选候选）
    /// 或落子后（如吃子、攻击、保护、对方应对）；说明某个局面时总是该局面。
    /// 界面据此决定能否直接画在当前棋盘上，避免把落子前的箭头叠在落子后的局面上。
    /// 早期保存的证据（如旧版 AI 讲解的证据快照）没有这一项，读取后为 `None`，表示所在局面未知：
    /// 界面不把它画在任何棋盘上，而不是猜测一个局面。
    #[serde(default)]
    pub on: Option<LineBase>,
}

/// 讲解某一着时，各类证据的格子与箭头所在的局面。
fn board_of(topic: &str) -> LineBase {
    match topic {
        "in_check" | "threat" | "quiet" | "development_need" | "escape" | "defend" | "unprotect" | "best" | "second"
        | "unsettled" | "contrast" | "candidate" => LineBase::Parent,
        _ => LineBase::Node,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub text: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    MultiPv,
    SearchMoves,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateView {
    pub mv: Move,
    pub notation: String,
    pub score: Score,
    pub bound: Bound,
    pub depth: u32,
    pub pv: Vec<Move>,
    pub pv_notation: Vec<String>,
    pub rank: Option<u32>,
    pub is_actual: bool,
    pub source: CandidateSource,
    /// 首着的直接作用（规则事实）。
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictTone {
    Best,
    Close,
    Inaccuracy,
    Mistake,
    Blunder,
    Unclear,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub tone: VerdictTone,
    pub label: String,
    /// 与首选的普通分差；涉及将杀分或边界分时为空。
    pub delta: Option<i32>,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplainLevel {
    /// 只有规则事实。
    Basic,
    /// 已有引擎证据，但缺少某一部分（候选不完整或实际走法未分析），结论不完整，可以重试。
    Partial,
    /// 候选比较与实际走法分析都已完成。
    Detailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveExplanation {
    pub generator_version: u32,
    pub level: ExplainLevel,
    pub mover: Color,
    pub mv: Move,
    pub notation: String,
    pub summary: String,
    pub need: Vec<Claim>,
    pub basis: Vec<Claim>,
    pub comparison: Vec<Claim>,
    pub replies: Vec<Claim>,
    pub lesson: String,
    pub verdict: Option<Verdict>,
    pub candidates: Vec<CandidateView>,
    pub evidence: Vec<Evidence>,
    pub warnings: Vec<String>,
    pub conditions: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionExplanation {
    pub generator_version: u32,
    pub side: Color,
    pub status: GameStatus,
    pub summary: String,
    pub claims: Vec<Claim>,
    pub evidence: Vec<Evidence>,
    pub candidates: Vec<CandidateView>,
    pub warnings: Vec<String>,
    pub conditions: Option<String>,
}

/// 一条引擎变化（评分为根局面行棋方视角）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineLine {
    pub mv: Move,
    pub score: Score,
    pub bound: Bound,
    pub depth: u32,
    pub pv: Vec<Move>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineEvidence {
    /// 多候选搜索结果，按名次排列。
    pub candidates: Vec<EngineLine>,
    /// 实际走法未进入候选时，用 searchmoves 单独搜索的结果。
    pub actual: Option<EngineLine>,
    /// 分析条件的文字说明（引擎、深度、时间、候选数等）。
    pub conditions: String,
    /// 候选来自同一轮完整搜索，且与引擎最终着法一致。
    #[serde(default)]
    pub complete: bool,
    /// 候选不完整或不一致的原因。
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplainContext {
    pub history_complete: bool,
    pub repetition_count: usize,
}

struct EvidenceSet {
    items: Vec<Evidence>,
    counters: [u32; 3],
    /// 讲解某一着（证据可能在落子前或落子后），还是说明一个局面（证据都在该局面上）。
    for_move: bool,
}

impl EvidenceSet {
    fn for_move() -> Self { Self { items: Vec::new(), counters: [0; 3], for_move: true } }

    fn for_position() -> Self { Self { items: Vec::new(), counters: [0; 3], for_move: false } }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self, kind: EvidenceKind, topic: &str, text: String, squares: Vec<Square>, arrows: Vec<Move>,
        line: Option<LineRef>, certainty: Certainty,
    ) -> String {
        let (prefix, slot) = match kind {
            EvidenceKind::Fact => ("f", 0),
            EvidenceKind::Variation => ("v", 1),
            EvidenceKind::Positional => ("p", 2),
        };
        self.counters[slot] += 1;
        let id = format!("{prefix}{}", self.counters[slot]);
        let on = Some(if self.for_move { board_of(topic) } else { LineBase::Node });
        self.items.push(Evidence { id: id.clone(), kind, topic: topic.to_string(), text, squares, arrows, line, certainty, on });
        id
    }
}

fn claim(text: impl Into<String>, ids: &[&String]) -> Claim {
    Claim { text: text.into(), evidence: ids.iter().map(|s| s.to_string()).collect() }
}

/// 从事实中提炼出的一条讲解要点。
struct Point {
    priority: u8,
    topic: &'static str,
    /// 用于一句话结论的短语。
    short: String,
    text: String,
    squares: Vec<Square>,
    arrows: Vec<Move>,
    kind: EvidenceKind,
    certainty: Certainty,
    /// 对走棋方有利（false 表示风险或代价）。
    positive: bool,
}

fn labels(board: &crate::board::Board, squares: &[Square]) -> String {
    join_labels(&squares.iter().map(|sq| label_at(board, *sq)).collect::<Vec<_>>())
}

/// 受威胁棋子的保护情况：每种吃法都能吃回、都吃不回，或只有部分吃法吃不回。
fn threat_detail(board: &crate::board::Board, threat: &facts::Threat) -> String {
    if threat.defended {
        "，攻击者价值更低".to_string()
    } else if threat.unanswered.len() == threat.attackers.len() {
        "，且没有保护".to_string()
    } else {
        format!("，{}吃下后无法吃回", labels(board, &threat.unanswered))
    }
}

/// 保护关系对应的具体吃子与回吃。
fn recapture_detail(board: &crate::board::Board, rel: &facts::Relation, protector: &str) -> String {
    if rel.against.is_empty() {
        String::new()
    } else {
        format!("：对方{}吃下后，{protector}可以吃回", labels(board, &rel.against))
    }
}

fn leg_detail(piece: Piece, from: Square, target: Square) -> String {
    let df = target.file() as i8 - from.file() as i8;
    let dr = target.rank() as i8 - from.rank() as i8;
    match piece.kind {
        PieceKind::Horse => {
            let leg = if dr.abs() == 2 { from.offset(0, dr.signum()) } else { from.offset(df.signum(), 0) };
            leg.map(|l| format!("（马腿 {l} 无子）")).unwrap_or_default()
        }
        PieceKind::Elephant => from.offset(df / 2, dr / 2).map(|e| format!("（象眼 {e} 无子）")).unwrap_or_default(),
        _ => String::new(),
    }
}

fn points(f: &MoveFacts) -> Vec<Point> {
    let before = &f.before.board;
    let after = &f.after.board;
    let mover = f.mover;
    let enemy = mover.opposite();
    let moved_before = piece_label(f.piece, f.mv.from);
    let moved_after = piece_label(f.piece, f.mv.to);
    let enemy_king = Piece::new(enemy, PieceKind::King).name();
    let mut out = Vec::new();
    let mut push = |priority, topic, short: String, text: String, squares, arrows, kind, certainty, positive| {
        out.push(Point { priority, topic, short, text, squares, arrows, kind, certainty, positive })
    };

    match f.opponent_status {
        GameStatus::Checkmate => {
            let mut squares = f.checkers.clone();
            squares.extend(after.find_king(enemy));
            push(
                100,
                "mate",
                "将死对方".into(),
                format!("走后{}将军，{}没有任何合法应着，被将死", labels(after, &f.checkers), enemy.label()),
                squares,
                vec![],
                EvidenceKind::Fact,
                Certainty::Certain,
                true,
            );
        }
        GameStatus::Stalemate => push(
            100,
            "stalemate",
            "使对方无子可走（困毙）".into(),
            format!("走后{}没有被将军，但也没有任何合法着法，即困毙，按规则{}判负", enemy.label(), enemy.label()),
            after.find_king(enemy).into_iter().collect(),
            vec![],
            EvidenceKind::Fact,
            Certainty::Certain,
            true,
        ),
        GameStatus::Ongoing => {}
    }

    if !f.checkers.is_empty() && f.opponent_status != GameStatus::Checkmate {
        let king_sq = after.find_king(enemy);
        let discovered = !f.checkers.contains(&f.mv.to);
        let text = if discovered {
            format!("{}让开线路后，{}将军对方{}（闪将）", moved_after, labels(after, &f.checkers), enemy_king)
        } else if f.checkers.len() > 1 {
            format!("{}将军，同时{}也形成将军（双将）", moved_after, labels(after, &f.checkers))
        } else {
            format!("{}直接将军对方{}", moved_after, enemy_king)
        };
        let mut squares = f.checkers.clone();
        squares.extend(king_sq);
        let arrows = king_sq.map(|k| f.checkers.iter().map(|c| Move::new(*c, k)).collect()).unwrap_or_default();
        push(90, "check", "将军".into(), text, squares, arrows, EvidenceKind::Fact, Certainty::Certain, true);
    }

    if let Some(resolution) = f.check_resolution {
        let checkers = rules::checkers(before, mover);
        let checker_text = labels(before, &checkers);
        let own_king = Piece::new(mover, PieceKind::King).name();
        let (short, text) = match resolution {
            CheckResolution::KingMove => {
                ("躲开将军".to_string(), format!("{}移动位置，躲开了{}的将军", f.piece.name(), checker_text))
            }
            CheckResolution::Capture => {
                ("吃掉将军的棋子".to_string(), format!("吃掉正在将军的{}，化解将军", checker_text))
            }
            CheckResolution::BlockLine => (
                "垫子挡住将军".to_string(),
                format!("{moved_after}挡在{checker_text}与{own_king}之间，切断了将军线路"),
            ),
            CheckResolution::BlockLeg => (
                "别马腿解将".to_string(),
                format!("{moved_after}别住了{checker_text}的马腿，马无法再将军"),
            ),
            CheckResolution::RemoveScreen => (
                "移开炮架解将".to_string(),
                format!("{moved_before}原本是{checker_text}与{own_king}之间唯一的炮架，移开后炮无法再隔子将军"),
            ),
            CheckResolution::AddScreen => (
                "增加阻隔解将".to_string(),
                format!("{moved_after}走到{checker_text}与{own_king}之间，炮前变成两个棋子，无法再隔子将军"),
            ),
            CheckResolution::Other => ("化解将军".to_string(), format!("这步化解了{checker_text}的将军")),
        };
        let mut squares = checkers;
        squares.push(f.mv.from);
        squares.push(f.mv.to);
        push(85, "escape", short, text, squares, vec![], EvidenceKind::Fact, Certainty::Certain, true);
    }

    if let Some(captured) = f.captured {
        let label = piece_label(captured, f.mv.to);
        push(
            80,
            "capture",
            format!("吃掉对方{label}"),
            format!("{}吃掉对方{}", moved_before, label),
            vec![f.mv.to],
            vec![f.mv],
            EvidenceKind::Fact,
            Certainty::Certain,
            true,
        );
    }

    let still_threatened = |sq: Square| f.threats_after.iter().any(|t| t.square == sq);
    let mut resolved_squares = Vec::new();
    for threat in &f.threats_before {
        let moved_away = threat.square == f.mv.from;
        let resolved = if moved_away { !still_threatened(f.mv.to) } else { !still_threatened(threat.square) };
        if !resolved {
            continue;
        }
        resolved_squares.push(threat.square);
        let label = piece_label(threat.piece, threat.square);
        let attackers = labels(before, &threat.attackers);
        let how = if moved_away {
            format!("本步把{label}移开，新位置没有受到同样的攻击")
        } else if threat.attackers.contains(&f.mv.to) {
            format!("吃掉了攻击{label}的{attackers}")
        } else if let Some(rel) = f.new_protects.iter().find(|r| r.target == threat.square) {
            format!(
                "{moved_after}在新位置保护了它{}{}",
                leg_detail(f.piece, f.mv.to, threat.square),
                recapture_detail(after, rel, &moved_after)
            )
        } else {
            format!("挡住或消除了对{label}的攻击")
        };
        let mut squares = vec![threat.square];
        squares.extend(threat.attackers.iter().copied());
        push(
            75,
            "defend",
            format!("化解{label}受攻击的问题"),
            format!("{label}原本受到{attackers}攻击，{how}"),
            squares,
            vec![],
            EvidenceKind::Fact,
            Certainty::Certain,
            true,
        );
    }

    let mover_value = f.piece.value_at(f.mv.to);
    let mut weak_targets = 0;
    for rel in &f.new_attacks {
        let label = piece_label(rel.target_piece, rel.target);
        let valuable = rel.target_piece.value_at(rel.target) > mover_value;
        if !rel.defended || valuable {
            let reason = if !rel.defended { "该子没有保护" } else { "该子价值高于攻击它的棋子" };
            push(
                70,
                "attack",
                format!("攻击对方{}{label}", if rel.defended { "" } else { "无保护的" }),
                format!("{moved_after}在新位置攻击对方{label}，{reason}"),
                vec![f.mv.to, rel.target],
                vec![Move::new(f.mv.to, rel.target)],
                EvidenceKind::Fact,
                Certainty::Certain,
                true,
            );
        } else if weak_targets == 0 {
            weak_targets += 1;
            push(
                30,
                "aim",
                format!("瞄准对方{label}"),
                format!("{moved_after}在新位置瞄准对方{label}（对方有保护）"),
                vec![f.mv.to, rel.target],
                vec![Move::new(f.mv.to, rel.target)],
                EvidenceKind::Fact,
                Certainty::Certain,
                true,
            );
        }
    }

    // 已作为“化解威胁”说明过的保护不再重复
    let mut protects: Vec<&facts::Relation> =
        f.new_protects.iter().filter(|r| !resolved_squares.contains(&r.target)).collect();
    protects.sort_by_key(|r| (!r.attacked, r.defended_before, -r.target_piece.value_at(r.target)));
    let important: Vec<&&facts::Relation> = protects.iter().filter(|r| r.attacked || !r.defended_before).take(2).collect();
    let chosen: Vec<&facts::Relation> =
        if important.is_empty() { protects.iter().take(1).copied().collect() } else { important.into_iter().copied().collect() };
    for rel in chosen {
        let label = piece_label(rel.target_piece, rel.target);
        let mut text = format!("{moved_after}在新位置保护{label}{}", leg_detail(f.piece, f.mv.to, rel.target));
        if !rel.defended_before {
            text.push_str("，该子此前没有保护");
        }
        if rel.attacked {
            text.push_str(&format!("，它正受到对方攻击{}", recapture_detail(after, rel, &moved_after)));
        }
        let priority = if rel.attacked {
            72
        } else if !rel.defended_before {
            60
        } else {
            25
        };
        push(
            priority,
            "protect",
            format!("增加对{label}的保护"),
            text,
            vec![f.mv.to, rel.target],
            vec![Move::new(f.mv.to, rel.target)],
            EvidenceKind::Fact,
            Certainty::Certain,
            true,
        );
    }

    for change in f.discovered.iter().take(2) {
        let line_piece = piece_label(change.piece, change.piece_square);
        let target = piece_label(change.target_piece, change.target);
        let weak = if change.target_defended { "" } else { "（无保护）" };
        let (short, text) = match change.via {
            LineVia::Screen => (
                format!("为{line_piece}架起炮架"),
                format!("{moved_after}成为{line_piece}的炮架，{line_piece}可以攻击对方{target}{weak}"),
            ),
            LineVia::Opened => (
                format!("打开{line_piece}的线路"),
                format!("{moved_before}让开线路后，{line_piece}可以攻击对方{target}{weak}"),
            ),
        };
        let priority = if change.target_defended { 35 } else { 65 };
        push(
            priority,
            "line",
            short,
            text,
            vec![change.piece_square, change.target, f.mv.to],
            vec![Move::new(change.piece_square, change.target)],
            EvidenceKind::Fact,
            Certainty::Certain,
            true,
        );
    }

    if let Some(dev) = f.development {
        let (short, text, kind, certainty) = match dev {
            Development::LeavesHome => (
                format!("出动{moved_before}"),
                format!("{moved_before}离开底线投入行动"),
                EvidenceKind::Fact,
                Certainty::Certain,
            ),
            Development::CrossesRiver => (
                format!("{}过河", f.piece.name()),
                format!("{}过河后可以横走，活动范围扩大", moved_before),
                EvidenceKind::Fact,
                Certainty::Certain,
            ),
            Development::CenterCannon => (
                "架中炮".to_string(),
                "炮移到中路，正对对方中路（通常配合车马进攻中路）".to_string(),
                EvidenceKind::Positional,
                Certainty::Tentative,
            ),
            Development::Defensive => (
                format!("{}巩固九宫", if f.piece.kind == PieceKind::Advisor { "补士" } else { "飞相" }),
                format!("{}离开底线，加强九宫附近的防守联系", f.piece.name()),
                EvidenceKind::Positional,
                Certainty::Tentative,
            ),
            Development::KingMove => (
                format!("动{}", f.piece.name()),
                format!("{}在九宫内移动", f.piece.name()),
                EvidenceKind::Fact,
                Certainty::Certain,
            ),
        };
        push(50, "develop", short, text, vec![f.mv.from, f.mv.to], vec![f.mv], kind, certainty, true);
    }

    // 风险与代价
    if let Some(threat) = f.threats_after.iter().find(|t| t.square == f.mv.to) {
        let detail = threat_detail(after, threat);
        push(
            78,
            "risk",
            format!("{moved_after}的落点受到攻击"),
            format!("{moved_after}的落点受到对方{}攻击{detail}", labels(after, &threat.attackers)),
            std::iter::once(f.mv.to).chain(threat.attackers.iter().copied()).collect(),
            threat.attackers.iter().map(|a| Move::new(*a, f.mv.to)).collect(),
            EvidenceKind::Fact,
            Certainty::Certain,
            false,
        );
    }
    for threat in &f.threats_after {
        let existed = f.threats_before.iter().any(|t| t.square == threat.square);
        if threat.square == f.mv.to || existed {
            continue;
        }
        let label = piece_label(threat.piece, threat.square);
        let detail = threat_detail(after, threat);
        push(
            70,
            "exposed",
            format!("{label}因此受到攻击"),
            format!("走后{label}受到对方{}攻击{detail}", labels(after, &threat.attackers)),
            std::iter::once(threat.square).chain(threat.attackers.iter().copied()).collect(),
            threat.attackers.iter().map(|a| Move::new(*a, threat.square)).collect(),
            EvidenceKind::Fact,
            Certainty::Certain,
            false,
        );
    }
    for rel in &f.lost_protects {
        if rel.attacked {
            continue; // 已在上面作为新受攻击的棋子说明
        }
        let label = piece_label(rel.target_piece, rel.target);
        push(
            40,
            "unprotect",
            format!("{label}失去保护"),
            format!("{moved_before}离开后，{label}失去了保护"),
            vec![f.mv.from, rel.target],
            vec![],
            EvidenceKind::Fact,
            Certainty::Certain,
            false,
        );
    }
    for change in f.enemy_gains.iter().take(2) {
        let enemy_piece = piece_label(change.piece, change.piece_square);
        let target = piece_label(change.target_piece, change.target);
        let text = match change.via {
            LineVia::Screen => format!("{moved_after}给对方{enemy_piece}做了炮架，对方{enemy_piece}可以攻击{target}"),
            LineVia::Opened => format!("{moved_before}让开线路后，对方{enemy_piece}可以攻击{target}"),
        };
        push(
            55,
            "enemy_line",
            format!("让对方{enemy_piece}获得攻击线路"),
            text,
            vec![change.piece_square, change.target],
            vec![Move::new(change.piece_square, change.target)],
            EvidenceKind::Fact,
            Certainty::Certain,
            false,
        );
    }

    if !out.iter().any(|p| p.positive && p.priority >= 40) {
        let (short, text) = match f.piece.kind {
            PieceKind::Pawn if f.mv.from.file() == f.mv.to.file() => (
                format!("挺{moved_before}"),
                format!("{moved_before}向前挺进一步；本步没有吃子或将军"),
            ),
            PieceKind::Pawn => {
                (format!("{moved_before}横移"), format!("{moved_before}横移一步；本步没有吃子或将军"))
            }
            _ => (
                format!("调整{moved_before}的位置"),
                format!(
                    "本步没有吃子或将军；{}的可走点由 {} 个变为 {} 个",
                    moved_before, f.mobility_before, f.mobility_after
                ),
            ),
        };
        out.push(Point {
            priority: 20,
            topic: "reposition",
            short,
            text,
            squares: vec![f.mv.from, f.mv.to],
            arrows: vec![f.mv],
            kind: EvidenceKind::Positional,
            certainty: Certainty::Tentative,
            positive: true,
        });
    }

    out.sort_by_key(|p| std::cmp::Reverse(p.priority));
    out
}

/// 主要作用的短语，供候选列表与一句话结论使用。
fn headline(points: &[Point]) -> String {
    let positives: Vec<&Point> = points.iter().filter(|p| p.positive).collect();
    let Some(first) = positives.first() else { return String::new() };
    let develop = positives.iter().find(|p| p.topic == "develop");
    // 非强制性的作用（保护、线路、攻击）时，以出子开头更自然
    let (lead, follow) = match develop {
        Some(dev) if first.priority < 75 && first.topic != "develop" => (*dev, Some(*first)),
        _ => (*first, positives.iter().find(|p| p.topic != first.topic).copied()),
    };
    match follow {
        Some(next) if next.topic != lead.topic && next.priority >= 40 => format!("{}，并{}", lead.short, next.short),
        _ => lead.short.clone(),
    }
}

fn first_move_summary(pos: &Position, mv: Move) -> String {
    facts::move_facts(pos, mv).map(|f| headline(&points(&f))).unwrap_or_default()
}

fn candidate_views(pos: &Position, engine: &EngineEvidence, actual: Option<Move>) -> Vec<CandidateView> {
    let mut out: Vec<CandidateView> = engine
        .candidates
        .iter()
        .enumerate()
        .map(|(i, line)| view_of(pos, line, Some(i as u32 + 1), actual == Some(line.mv), CandidateSource::MultiPv))
        .collect();
    if let Some(line) = &engine.actual
        && !out.iter().any(|c| c.mv == line.mv)
    {
        out.push(view_of(pos, line, None, true, CandidateSource::SearchMoves));
    }
    out
}

/// 把引擎候选整理成界面展示用的列表（含中文记谱与首着的直接作用）。
pub fn candidate_list(pos: &Position, lines: &[EngineLine]) -> Vec<CandidateView> {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| view_of(pos, line, Some(i as u32 + 1), false, CandidateSource::MultiPv))
        .collect()
}

/// 变化中的着法是否依次都合法。
pub fn line_is_legal(pos: &Position, moves: &[Move]) -> bool {
    let mut cur = pos.clone();
    for mv in moves {
        if !rules::is_legal(&cur, *mv) {
            return false;
        }
        cur = cur.after(*mv);
    }
    true
}

/// 候选的主变化不能合法回放时的说明（变化已从候选中移除）。
fn illegal_line_warnings(pos: &Position, engine: &EngineEvidence) -> Vec<String> {
    engine
        .candidates
        .iter()
        .chain(engine.actual.iter())
        .filter(|l| !line_is_legal(pos, &l.pv))
        .map(|l| {
            let name = notation::move_to_chinese(pos, l.mv).unwrap_or_else(|_| l.mv.iccs());
            format!("候选{name}的主变化无法在当前局面合法回放，已移除")
        })
        .collect()
}

/// 候选进入展示数据前统一检查主变化：不能合法回放的变化整条移除，不提供演示。
fn view_of(pos: &Position, line: &EngineLine, rank: Option<u32>, is_actual: bool, source: CandidateSource) -> CandidateView {
    let pv = if line_is_legal(pos, &line.pv) { line.pv.clone() } else { Vec::new() };
    CandidateView {
        mv: line.mv,
        notation: notation::move_to_chinese(pos, line.mv).unwrap_or_else(|_| line.mv.iccs()),
        score: line.score,
        bound: line.bound,
        depth: line.depth,
        pv_notation: notation::line_to_chinese(pos, &pv),
        pv,
        rank,
        is_actual,
        source,
        summary: first_move_summary(pos, line.mv),
    }
}

fn exact_cp(line: &EngineLine) -> Option<i32> { if line.bound == Bound::Exact { line.score.cp() } else { None } }

/// 普通分的取值区间：精确分为单点，下界只知道不低于该值，上界只知道不高于该值。
fn cp_interval(line: &EngineLine) -> Option<(Option<i32>, Option<i32>)> {
    let cp = line.score.cp()?;
    Some(match line.bound {
        Bound::Exact => (Some(cp), Some(cp)),
        Bound::Lower => (Some(cp), None),
        Bound::Upper => (None, Some(cp)),
    })
}

fn unstable(note: &str) -> Verdict {
    Verdict { tone: VerdictTone::Unclear, label: "比较尚不稳定".into(), delta: None, note: note.to_string() }
}

fn severity(loss: i32) -> (VerdictTone, &'static str) {
    match loss {
        i32::MIN..=30 => (VerdictTone::Close, "与首选接近"),
        31..=80 => (VerdictTone::Inaccuracy, "略逊于首选"),
        81..=200 => (VerdictTone::Mistake, "明显不如首选"),
        _ => (VerdictTone::Blunder, "严重失误"),
    }
}

/// 比较实际走法与首选。
///
/// 将杀分与普通分不相减；涉及搜索边界值时把评分当作区间，只给出区间关系能够证明的结论，
/// 证明不了时标记为“比较尚不稳定”。“首选”只表示搜索排名，不代表其他走法一定有失误。
pub fn judge(best: &EngineLine, actual: &EngineLine, actual_rank: Option<u32>) -> Verdict {
    if actual_rank == Some(1) || best.mv == actual.mv {
        return Verdict { tone: VerdictTone::Best, label: "当前分析的首选".into(), delta: None, note: String::new() };
    }
    let exact = best.bound == Bound::Exact && actual.bound == Bound::Exact;
    if best.score.is_mate() || actual.score.is_mate() {
        if !exact {
            return unstable("涉及将杀分的搜索边界值，需要继续搜索才能比较");
        }
        let verdict = |tone, label: &str| Verdict { tone, label: label.to_string(), delta: None, note: String::new() };
        return match (best.score, actual.score) {
            (Score::Mate(b), Score::Mate(a)) if b > 0 && a > 0 => {
                if a <= b + 1 {
                    verdict(VerdictTone::Close, "同样可以形成杀棋")
                } else {
                    verdict(VerdictTone::Inaccuracy, "也能杀棋，但比首选更慢")
                }
            }
            (Score::Mate(b), _) if b > 0 => verdict(VerdictTone::Blunder, "错过了杀棋"),
            (_, Score::Mate(a)) if a <= 0 => match best.score {
                Score::Mate(b) if b <= 0 => verdict(VerdictTone::Close, "两者都难以避免被杀"),
                _ => verdict(VerdictTone::Blunder, "会导致被对方杀棋"),
            },
            (Score::Mate(_), Score::Cp(_)) => verdict(VerdictTone::Close, "首选同样处于被杀局面，本步至少拖延"),
            _ => unstable("两次搜索的将杀判断不一致"),
        };
    }
    let (Some((best_lo, best_hi)), Some((actual_lo, actual_hi))) = (cp_interval(best), cp_interval(actual)) else {
        return unstable("评分无法比较");
    };
    if exact {
        let d = best_lo.unwrap_or(0) - actual_lo.unwrap_or(0);
        if d < -20 {
            return unstable("单独分析该着法的得分高于首选，可能因搜索预算不同，需要增加搜索预算");
        }
        let d = d.max(0);
        let (tone, label) = severity(d);
        return Verdict { tone, label: label.to_string(), delta: Some(d), note: String::new() };
    }
    // 损失 = 首选 - 实际，可证明的范围是 [首选下界 - 实际上界, 首选上界 - 实际下界]
    let min_loss = best_lo.zip(actual_hi).map(|(b, a)| b - a);
    let max_loss = best_hi.zip(actual_lo).map(|(b, a)| b - a);
    match (min_loss, max_loss) {
        (Some(m), _) if m > 30 => {
            let (tone, label) = severity(m);
            Verdict {
                tone,
                label: label.to_string(),
                delta: None,
                note: format!("按搜索边界值，至少比首选低约 {m} 分"),
            }
        }
        (_, Some(m)) if m <= 30 => Verdict {
            tone: VerdictTone::Close,
            label: "与首选接近".into(),
            delta: None,
            note: format!("按搜索边界值，最多比首选低约 {} 分", m.max(0)),
        },
        _ => unstable("评分是搜索边界值，区间相互重叠，无法确定差距"),
    }
}

fn score_text(line: &EngineLine, side: Color) -> String { score_label(line.score, line.bound, side) }

/// 评分说明（`side` 为评分所属的一方，即搜索根局面的行棋方）。
/// 边界值只是搜索给出的界限：普通分注明“不低于/不高于”，将杀分不据此断言杀棋。
pub fn score_label(score: Score, bound: Bound, side: Color) -> String {
    let who = side.label();
    match (score, bound) {
        (Score::Cp(cp), Bound::Exact) => format!("{who}视角评分 {cp:+}"),
        (Score::Cp(cp), Bound::Lower) => format!("{who}视角评分不低于 {cp:+}（搜索边界值）"),
        (Score::Cp(cp), Bound::Upper) => format!("{who}视角评分不高于 {cp:+}（搜索边界值）"),
        (Score::Mate(_), Bound::Lower | Bound::Upper) => "搜索出现将杀边界值，尚需继续确认".to_string(),
        (Score::Mate(n), Bound::Exact) if n > 0 => format!("{who}可在 {n} 步内杀棋"),
        (Score::Mate(0), Bound::Exact) => format!("{who}已无着可走"),
        (Score::Mate(n), Bound::Exact) => format!("{who} {} 步内会被杀", -n),
    }
}

/// 某条变化的将杀结论：精确将杀分给出步数，边界值只说明尚需确认；普通分没有将杀结论。
/// 返回 (主题, 文字, 是否为走棋方被杀, 是否已确认)。
fn mate_finding(line: &EngineLine, side: Color) -> Option<(&'static str, String, bool, bool)> {
    let who = side.label();
    match (line.score, line.bound) {
        (Score::Cp(_), _) | (Score::Mate(0), _) => None,
        (Score::Mate(_), Bound::Lower | Bound::Upper) => Some((
            "mate_bound",
            "这条变化的搜索出现将杀边界值，是否形成杀棋尚需继续搜索确认".to_string(),
            false,
            false,
        )),
        (Score::Mate(n), Bound::Exact) if n > 0 => Some(("mate_line", format!("按主变化，{who}可在 {n} 步内杀棋"), false, true)),
        (Score::Mate(n), Bound::Exact) => Some(("mated_line", format!("按主变化，{who} {} 步内会被杀", -n), true, true)),
    }
}

fn lesson_for(points: &[Point], verdict: Option<&Verdict>, reply_captures: bool, f: &MoveFacts) -> String {
    let has = |topic: &str| points.iter().any(|p| p.topic == topic);
    if let Some(v) = verdict {
        match v.tone {
            VerdictTone::Blunder if v.label.contains("错过了杀棋") => {
                return "有将军机会时，先计算强制性的将军变化，再考虑其他着法。".into();
            }
            VerdictTone::Blunder | VerdictTone::Mistake if reply_captures || has("risk") || has("exposed") => {
                return "走子前先检查：落点和失去保护的棋子会不会被对方吃掉，对方吃完后自己能否吃回。".into();
            }
            VerdictTone::Blunder | VerdictTone::Mistake => {
                return "在多个候选之间比较时，先看对方最有力的回应，而不只看自己这一步的直接作用。".into();
            }
            _ => {}
        }
    }
    if f.was_in_check {
        "被将军时比较三种应将方式：动将、垫子、吃掉将军的棋子，并看应将后哪些棋子更安全。".into()
    } else if has("mate") || has("stalemate") {
        "收官时注意对方将帅的出路：控制它的每一个可走点，就能形成杀棋或困毙。".into()
    } else if has("capture") {
        "吃子前检查对方能否反吃或借机反击，再决定是否值得。".into()
    } else if has("check") {
        "将军前先想好后续手段；没有后续的将军可能只是浪费步数。".into()
    } else if has("protect") || has("develop") {
        "选择出子位置时，同时检查新增了哪些攻击和保护关系。".into()
    } else if has("risk") || has("exposed") || has("unprotect") {
        "每走一步后都检查：自己哪些棋子受到攻击、哪些失去了保护。".into()
    } else {
        "没有直接战术时，优先让活动范围小的棋子参与配合。".into()
    }
}

/// 讲解某一着。`engine` 为空时只生成基于规则事实的基本讲解。
pub fn explain_move(
    before: &Position, mv: Move, ctx: &ExplainContext, engine: Option<&EngineEvidence>,
) -> Option<MoveExplanation> {
    if !rules::is_legal(before, mv) {
        return None;
    }
    let f = facts::move_facts(before, mv)?;
    let pts = points(&f);
    let mut ev = EvidenceSet::for_move();
    let mut warnings = Vec::new();
    if !ctx.history_complete {
        warnings.push("此前的着法历史不完整，长将、长捉等依赖历史的判定可能不准确".to_string());
    }

    // 当前需求
    let mut need = Vec::new();
    if f.was_in_check {
        let checkers = rules::checkers(&before.board, f.mover);
        let id = ev.add(
            EvidenceKind::Fact,
            "in_check",
            format!("{}正被{}将军", f.mover.label(), labels(&before.board, &checkers)),
            checkers.clone(),
            vec![],
            None,
            Certainty::Certain,
        );
        need.push(claim(format!("{}正被将军，必须先应将", f.mover.label()), &[&id]));
    }
    for threat in f.threats_before.iter().take(2) {
        let label = piece_label(threat.piece, threat.square);
        let detail = threat_detail(&before.board, threat);
        let text = format!("{label}受到对方{}攻击{detail}", labels(&before.board, &threat.attackers));
        let id = ev.add(
            EvidenceKind::Fact,
            "threat",
            text.clone(),
            std::iter::once(threat.square).chain(threat.attackers.iter().copied()).collect(),
            threat.attackers.iter().map(|a| Move::new(*a, threat.square)).collect(),
            None,
            Certainty::Certain,
        );
        need.push(claim(text, &[&id]));
    }
    if need.is_empty() {
        let id = ev.add(
            EvidenceKind::Fact,
            "quiet",
            "当前没有被将军，也没有受攻击而缺少保护的棋子".into(),
            vec![],
            vec![],
            None,
            Certainty::Certain,
        );
        let home: Vec<String> = before
            .board
            .pieces()
            .filter(|(sq, p)| {
                p.color == f.mover
                    && matches!(p.kind, PieceKind::Rook | PieceKind::Horse)
                    && sq.rank() == f.mover.home_rank()
            })
            .map(|(sq, p)| piece_label(p, sq))
            .collect();
        if home.len() >= 2 {
            let pid = ev.add(
                EvidenceKind::Positional,
                "development_need",
                format!("{}仍在底线，尚未出动", join_labels(&home)),
                vec![],
                vec![],
                None,
                Certainty::Tentative,
            );
            need.push(claim("当前没有直接威胁，主要任务是尽快出动尚在底线的车马", &[&id, &pid]));
        } else {
            need.push(claim("当前没有直接威胁，可以着手改善子力位置或寻找攻击目标", &[&id]));
        }
    }

    // 具体依据：重要的要点全部列出，次要要点只在内容不足时补充
    let mut basis = Vec::new();
    let important = pts.iter().filter(|p| p.priority >= 40).count();
    for p in pts.iter().filter(|p| p.priority >= 40 || important < 2).take(6) {
        let id = ev.add(p.kind, p.topic, p.text.clone(), p.squares.clone(), p.arrows.clone(), None, p.certainty);
        let text = if p.positive { p.text.clone() } else { format!("需要注意：{}", p.text) };
        basis.push(claim(text, &[&id]));
    }

    // 候选比较、主要应对与评价
    let mut comparison = Vec::new();
    let mut replies = Vec::new();
    let mut verdict = None;
    let mut candidates = Vec::new();
    let mut reply_captures = false;
    let mut swing_loss: Option<String> = None;
    let mut conditions = None;
    let mut level = if engine.is_some() { ExplainLevel::Partial } else { ExplainLevel::Basic };

    if let Some(engine) = engine.filter(|e| !e.candidates.is_empty()) {
        conditions = Some(engine.conditions.clone());
        warnings.extend(engine.notes.iter().cloned());
        candidates = candidate_views(before, engine, Some(mv));
        warnings.extend(illegal_line_warnings(before, engine));
        let best = &engine.candidates[0];
        let rank = engine.candidates.iter().position(|c| c.mv == mv).map(|i| i as u32 + 1);
        let actual_line = rank.map(|r| &engine.candidates[r as usize - 1]).or(engine.actual.as_ref());
        let best_cn = notation::move_to_chinese(before, best.mv).unwrap_or_default();

        if !engine.complete {
            // 候选不完整，或与引擎最终着法不一致：排名与分差都不可靠，不评价优劣，也不称哪一着为首选
            let reason =
                if engine.notes.is_empty() { "引擎没有给出一轮完整一致的候选".to_string() } else { engine.notes.join("；") };
            let id = ev.add(
                EvidenceKind::Variation,
                "unsettled",
                format!("本次分析尚未完成：{reason}"),
                vec![],
                vec![],
                None,
                Certainty::Tentative,
            );
            comparison.push(claim(
                format!("本次分析尚未完成，暂不比较{}与其他着法的优劣；可以重试或深入分析", f.notation),
                &[&id],
            ));
            verdict = Some(Verdict { tone: VerdictTone::Unclear, label: "比较尚未完成".into(), delta: None, note: reason });
            warnings.push("候选比较尚未完成：暂不评价优劣，引擎变化只作参考".to_string());
        } else if let Some(actual) = actual_line {
            let v = judge(best, actual, rank);
            let best_id = ev.add(
                EvidenceKind::Variation,
                "best",
                format!("引擎首选{}（{}，深度 {}）", best_cn, score_text(best, f.mover), best.depth),
                vec![best.mv.from, best.mv.to],
                vec![best.mv],
                Some(LineRef {
                    base: LineBase::Parent,
                    moves: best.pv.clone(),
                    notation: notation::line_to_chinese(before, &best.pv),
                }),
                Certainty::Supported,
            );
            if rank.is_none() {
                comparison.push(claim(
                    format!("{}没有进入前 {} 个候选，已在相同条件下单独分析", f.notation, engine.candidates.len()),
                    &[&best_id],
                ));
            }
            if v.tone == VerdictTone::Best {
                let mut text = format!("在当前分析中，{}是首选", f.notation);
                let mut ids = vec![best_id.clone()];
                if let Some(second) = engine.candidates.get(1) {
                    let second_cn = notation::move_to_chinese(before, second.mv).unwrap_or_default();
                    let second_id = ev.add(
                        EvidenceKind::Variation,
                        "second",
                        format!("第二候选{}（{}）", second_cn, score_text(second, f.mover)),
                        vec![second.mv.from, second.mv.to],
                        vec![second.mv],
                        Some(LineRef {
                            base: LineBase::Parent,
                            moves: second.pv.clone(),
                            notation: notation::line_to_chinese(before, &second.pv),
                        }),
                        Certainty::Supported,
                    );
                    ids.push(second_id);
                    match (exact_cp(best), exact_cp(second)) {
                        (Some(b), Some(s)) if b - s <= 30 => text.push_str(&format!(
                            "；与第二候选{second_cn}相差约 {} 分，在当前分析预算下两者接近",
                            b - s
                        )),
                        (Some(b), Some(s)) => text.push_str(&format!("；比第二候选{second_cn}高约 {} 分", b - s)),
                        _ => text.push_str(&format!("；第二候选是{second_cn}")),
                    }
                    let second_summary = first_move_summary(before, second.mv);
                    if !second_summary.is_empty() {
                        text.push_str(&format!("（{second_cn}的直接作用：{second_summary}）"));
                    }
                }
                let refs: Vec<&String> = ids.iter().collect();
                comparison.push(claim(text, &refs));
            } else {
                let best_summary = first_move_summary(before, best.mv);
                let mut text = format!("引擎首选{best_cn}");
                if !best_summary.is_empty() {
                    text.push_str(&format!("（{best_summary}）"));
                }
                match v.delta {
                    Some(d) => text.push_str(&format!("；{}比它低约 {d} 分，{}", f.notation, v.label)),
                    None => text.push_str(&format!("；相比之下{}{}", f.notation, v.label)),
                }
                if !v.note.is_empty() {
                    text.push_str(&format!("（{}）", v.note));
                }
                comparison.push(claim(text, &[&best_id]));
            }

            if v.tone == VerdictTone::Unclear {
                warnings.push("不同搜索得出的结论不一致，建议用“深入分析”增加搜索预算".to_string());
            }
            verdict = Some(v);
        } else {
            warnings.push(format!("{}没有进入候选，单独分析未完成，暂时无法比较优劣", f.notation));
        }

        if let Some(actual) = actual_line {
            // 分析不完整时，实际走法的变化只作参考
            let (lead, certainty) =
                if engine.complete { ("", Certainty::Supported) } else { ("按尚未完成的搜索，", Certainty::Tentative) };
            // 主要应对：实际走法主变化中的对方回应
            if actual.pv.len() >= 2 && actual.pv[0] == mv {
                let reply = actual.pv[1];
                let reply_cn = notation::move_to_chinese(&f.after, reply).unwrap_or_else(|_| reply.iccs());
                reply_captures = f.after.board.get(reply.to).is_some();
                let reply_effect = first_move_summary(&f.after, reply);
                let line = LineRef {
                    base: LineBase::Parent,
                    moves: actual.pv.clone(),
                    notation: notation::line_to_chinese(before, &actual.pv),
                };
                let text = if reply_effect.is_empty() {
                    format!("{lead}对方主要应对{reply_cn}")
                } else {
                    format!("{lead}对方主要应对{reply_cn}：{reply_effect}")
                };
                let id = ev.add(
                    EvidenceKind::Variation,
                    "reply",
                    format!("{text}（主变化：{}）", line.notation.join(" ")),
                    vec![reply.from, reply.to],
                    vec![reply],
                    Some(line),
                    certainty,
                );
                replies.push(claim(text, &[&id]));

                let (swing, played) = facts::material_swing(before, &actual.pv, 6);
                if played >= 2 && swing.abs() >= 100 {
                    let phrase = material_phrase(swing);
                    let text = if swing < 0 {
                        format!("{lead}按这条变化走 {played} 步后，{}净损失子力{phrase}", f.mover.label())
                    } else {
                        format!("{lead}按这条变化走 {played} 步后，{}净得子力{phrase}", f.mover.label())
                    };
                    if swing < 0 {
                        swing_loss = Some(format!("按主要变化会净损失子力{phrase}"));
                    }
                    let sid = ev.add(
                        EvidenceKind::Variation,
                        "material",
                        text.clone(),
                        vec![],
                        vec![],
                        Some(LineRef {
                            base: LineBase::Parent,
                            moves: actual.pv.iter().take(played).copied().collect(),
                            notation: notation::line_to_chinese(before, &actual.pv[..played]),
                        }),
                        certainty,
                    );
                    replies.push(claim(text, &[&sid]));
                }
            }
            if let Some((topic, text, mated, confirmed)) = mate_finding(actual, f.mover) {
                let text = format!("{lead}{text}");
                let certainty = if confirmed { certainty } else { Certainty::Tentative };
                let id = ev.add(EvidenceKind::Variation, topic, text.clone(), vec![], vec![], None, certainty);
                replies.push(claim(text, &[&id]));
                if mated && confirmed {
                    swing_loss.get_or_insert_with(|| "会被对方杀棋".to_string());
                }
            }
        }
        // 只有候选完整一致、且实际走法也有分析时，才算详细讲解
        if engine.complete && actual_line.is_some() {
            level = ExplainLevel::Detailed;
        }
    } else if engine.is_none() {
        // 无引擎时，用同一棋子的其他走法做事实层面的对照
        if let Some(text) = fact_contrast(before, &f, &pts) {
            let id = ev.add(EvidenceKind::Fact, "contrast", text.clone(), vec![], vec![], None, Certainty::Certain);
            comparison.push(claim(text, &[&id]));
        }
    } else {
        warnings.push("引擎没有返回候选，暂时无法比较优劣".to_string());
    }

    // 一句话结论
    let head = headline(&pts);
    let summary = match verdict.as_ref().map(|v| v.tone) {
        Some(VerdictTone::Mistake | VerdictTone::Blunder) => {
            let cost = swing_loss
                .clone()
                .or_else(|| pts.iter().find(|p| !p.positive && p.priority >= 55).map(|p| p.short.clone()))
                .unwrap_or_else(|| verdict.as_ref().map(|v| v.label.clone()).unwrap_or_default());
            format!("{}：{}，但{}。", f.notation, head, cost)
        }
        _ => {
            let risk = pts.iter().find(|p| !p.positive && p.priority >= 70);
            match risk {
                Some(r) => format!("{}：{}，但{}。", f.notation, head, r.short),
                None => format!("{}：{}。", f.notation, head),
            }
        }
    };

    let lesson = lesson_for(&pts, verdict.as_ref(), reply_captures, &f);
    let mut explanation = MoveExplanation {
        generator_version: GENERATOR_VERSION,
        level,
        mover: f.mover,
        mv,
        notation: f.notation.clone(),
        summary,
        need,
        basis,
        comparison,
        replies,
        lesson,
        verdict,
        candidates,
        evidence: ev.items,
        warnings,
        conditions,
    };
    audit(&mut explanation.evidence, &mut explanation.warnings, before, &f.after);
    for claims in [&mut explanation.need, &mut explanation.basis, &mut explanation.comparison, &mut explanation.replies] {
        drop_unknown_refs(claims, &explanation.evidence);
    }
    Some(explanation)
}

/// 与同一棋子另一种走法对照，说明本步在事实层面多做了什么。
fn fact_contrast(before: &Position, f: &MoveFacts, pts: &[Point]) -> Option<String> {
    let mine: Vec<&Point> = pts.iter().filter(|p| p.positive && p.priority >= 40).collect();
    if mine.is_empty() {
        return None;
    }
    for alt in rules::legal_moves_from(before, f.mv.from) {
        if alt == f.mv {
            continue;
        }
        let Some(af) = facts::move_facts(before, alt) else { continue };
        let alt_pts = points(&af);
        let missing: Vec<&&Point> =
            mine.iter().filter(|p| !alt_pts.iter().any(|a| a.topic == p.topic && a.short == p.short)).collect();
        if let Some(diff) = missing.first() {
            let alt_cn = notation::move_to_chinese(before, alt).ok()?;
            return Some(format!(
                "与{alt_cn}相比，{}能{}，{alt_cn}没有这一作用；两者的后续差别可以查看候选变化",
                f.notation, diff.short
            ));
        }
    }
    None
}

/// 检查证据中的变化能否合法回放、坐标是否有效；不通过的变化会被移除并记录警告。
fn audit(evidence: &mut [Evidence], warnings: &mut Vec<String>, parent: &Position, node: &Position) {
    const OVERCLAIM: [&str; 5] = ["唯一", "必胜", "必然", "无论怎么走", "一定能"];
    for e in evidence.iter_mut() {
        if let Some(line) = &e.line {
            let start = match line.base {
                LineBase::Parent => parent,
                LineBase::Node => node,
            };
            if !line_is_legal(start, &line.moves) {
                warnings.push(format!("证据 {} 的变化无法在当前局面合法回放，已移除", e.id));
                e.line = None;
            }
        }
        if OVERCLAIM.iter().any(|w| e.text.contains(w)) && e.kind != EvidenceKind::Fact {
            warnings.push(format!("证据 {} 的措辞超出了证据强度", e.id));
        }
    }
}

fn drop_unknown_refs(claims: &mut [Claim], evidence: &[Evidence]) {
    for c in claims.iter_mut() {
        c.evidence.retain(|id| evidence.iter().any(|e| &e.id == id));
    }
}

/// 说明一个局面：用于根节点、导入的残局，以及任意节点的“当前局面”。
pub fn explain_position(pos: &Position, ctx: &ExplainContext, engine: Option<&EngineEvidence>) -> PositionExplanation {
    let pf = facts::position_facts(pos);
    let mut ev = EvidenceSet::for_position();
    let mut claims = Vec::new();
    let mut warnings = Vec::new();
    if !ctx.history_complete {
        warnings.push("此局面之前的着法未知，长将、长捉等依赖历史的判定可能不完整".to_string());
    }
    let side = pos.side;
    let king = pos.board.find_king(side);

    let summary = match pf.status {
        GameStatus::Checkmate => {
            let id = ev.add(
                EvidenceKind::Fact,
                "mated",
                format!("{}将军，{}没有任何合法应着", labels(&pos.board, &pf.checkers), side.label()),
                pf.checkers.iter().copied().chain(king).collect(),
                vec![],
                None,
                Certainty::Certain,
            );
            claims.push(claim(format!("{}已被将死", side.label()), &[&id]));
            format!("{}已被将死，对局结束。", side.label())
        }
        GameStatus::Stalemate => {
            let reasons = king
                .map(|k| {
                    rules::pseudo_targets(&pos.board, k)
                        .into_iter()
                        .filter_map(|t| rules::illegal_reason(pos, Move::new(k, t)).map(|r| format!("去 {t}：{}", r.message)))
                        .take(4)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let mut text = format!("{}没有被将军，但所有棋子都没有合法着法", side.label());
            if !reasons.is_empty() {
                text.push_str(&format!("；{}的可走点都不安全：{}", Piece::new(side, PieceKind::King).name(), reasons.join("；")));
            }
            let id = ev.add(
                EvidenceKind::Fact,
                "stalemated",
                text.clone(),
                king.into_iter().collect(),
                vec![],
                None,
                Certainty::Certain,
            );
            claims.push(claim(text, &[&id]));
            format!("{}无子可走（困毙），按规则判负，对局结束。", side.label())
        }
        GameStatus::Ongoing => {
            let check = if pf.in_check { "，正被将军" } else { "" };
            format!(
                "轮到{}走{}。子力：红方{}；黑方{}。",
                side.label(),
                check,
                facts::material_inventory(&pos.board, Color::Red),
                facts::material_inventory(&pos.board, Color::Black)
            )
        }
    };

    if pf.status == GameStatus::Ongoing {
        if pf.in_check {
            let id = ev.add(
                EvidenceKind::Fact,
                "in_check",
                format!("{}正在将军{}", labels(&pos.board, &pf.checkers), side.label()),
                pf.checkers.iter().copied().chain(king).collect(),
                king.map(|k| pf.checkers.iter().map(|c| Move::new(*c, k)).collect()).unwrap_or_default(),
                None,
                Certainty::Certain,
            );
            claims.push(claim(format!("{}正被将军，必须先应将（共有 {} 种应将着法）", side.label(), pf.legal_moves), &[&id]));
        } else {
            let id = ev.add(
                EvidenceKind::Fact,
                "mobility",
                format!("{}共有 {} 种合法着法", side.label(), pf.legal_moves),
                vec![],
                vec![],
                None,
                Certainty::Certain,
            );
            claims.push(claim(format!("{}共有 {} 种合法着法", side.label(), pf.legal_moves), &[&id]));
        }
        for (threats, own) in [(&pf.own_threats, true), (&pf.opponent_threats, false)] {
            for t in threats.iter().take(3) {
                let label = piece_label(t.piece, t.square);
                let detail = threat_detail(&pos.board, t);
                let owner = if own { side.label() } else { side.opposite().label() };
                let text = format!("{owner}的{label}受到{}攻击{detail}", labels(&pos.board, &t.attackers));
                let id = ev.add(
                    EvidenceKind::Fact,
                    if own { "own_threat" } else { "target" },
                    text.clone(),
                    std::iter::once(t.square).chain(t.attackers.iter().copied()).collect(),
                    t.attackers.iter().map(|a| Move::new(*a, t.square)).collect(),
                    None,
                    Certainty::Certain,
                );
                let lead = if own { "需要处理：" } else { "可以利用：" };
                claims.push(claim(format!("{lead}{text}"), &[&id]));
            }
        }
        let diff = pf.material_red - pf.material_black;
        if diff.abs() >= 100 {
            let leader = if diff > 0 { Color::Red } else { Color::Black };
            let id = ev.add(
                EvidenceKind::Fact,
                "material",
                format!("按粗略子力价值，{}多出{}", leader.label(), material_phrase(diff)),
                vec![],
                vec![],
                None,
                Certainty::Certain,
            );
            claims.push(claim(format!("按粗略子力价值，{}多出{}", leader.label(), material_phrase(diff)), &[&id]));
        }
        if ctx.repetition_count > 1 {
            let id = ev.add(
                EvidenceKind::Fact,
                "repetition",
                format!("当前局面在本段棋谱中已出现 {} 次", ctx.repetition_count),
                vec![],
                vec![],
                None,
                Certainty::Certain,
            );
            claims.push(claim(format!("当前局面在本段棋谱中已出现 {} 次，注意重复局面规则", ctx.repetition_count), &[&id]));
        }
    }

    let mut candidates = Vec::new();
    let mut conditions = None;
    if let Some(engine) = engine.filter(|e| !e.candidates.is_empty()) {
        conditions = Some(engine.conditions.clone());
        candidates = candidate_views(pos, engine, None);
        warnings.extend(illegal_line_warnings(pos, engine));
        warnings.extend(engine.notes.iter().cloned());
    }
    if let Some(engine) = engine.filter(|e| !e.candidates.is_empty() && !e.complete) {
        // 与逐着讲解相同：候选不完整或与最终着法不一致时，不确定首选
        let id = ev.add(
            EvidenceKind::Variation,
            "unsettled",
            format!("本次分析尚未完成：{}", engine.notes.join("；")),
            vec![],
            vec![],
            None,
            Certainty::Tentative,
        );
        claims.push(claim("本次分析没有得到一轮完整一致的候选，暂不确定首选；可以重新分析或深入分析", &[&id]));
    } else if let Some(engine) = engine.filter(|e| !e.candidates.is_empty()) {
        let best = &engine.candidates[0];
        let best_cn = notation::move_to_chinese(pos, best.mv).unwrap_or_default();
        let effect = first_move_summary(pos, best.mv);
        let line = LineRef {
            base: LineBase::Node,
            moves: best.pv.clone(),
            notation: notation::line_to_chinese(pos, &best.pv),
        };
        let mut text = format!("引擎首选{best_cn}（{}）", score_text(best, side));
        if !effect.is_empty() {
            text.push_str(&format!("：{effect}"));
        }
        let id = ev.add(
            EvidenceKind::Variation,
            "best",
            format!("{text}；主变化 {}", line.notation.join(" ")),
            vec![best.mv.from, best.mv.to],
            vec![best.mv],
            Some(line),
            Certainty::Supported,
        );
        claims.push(claim(text, &[&id]));
    }

    let mut evidence = ev.items;
    audit(&mut evidence, &mut warnings, pos, pos);
    drop_unknown_refs(&mut claims, &evidence);
    PositionExplanation {
        generator_version: GENERATOR_VERSION,
        side,
        status: pf.status,
        summary,
        claims,
        evidence,
        candidates,
        warnings,
        conditions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_position;

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    const CTX: ExplainContext = ExplainContext { history_complete: true, repetition_count: 1 };

    fn line(m: &str, score: Score, pv: &[&str]) -> EngineLine {
        EngineLine { mv: mv(m), score, bound: Bound::Exact, depth: 12, pv: pv.iter().map(|s| mv(s)).collect() }
    }

    #[test]
    fn horse_development_example() {
        let e = explain_move(&Position::startpos(), mv("h0g2"), &CTX, None).unwrap();
        assert_eq!(e.summary, "马二进三：出动右马，并增加对中兵的保护。");
        assert_eq!(e.level, ExplainLevel::Basic);
        assert!(e.basis.iter().any(|c| c.text.contains("保护中兵") && c.text.contains("马腿 f2 无子")), "{:?}", e.basis);
        assert!(e.comparison.iter().any(|c| c.text.contains("马二进一")), "{:?}", e.comparison);
        assert!(e.lesson.contains("保护关系"));
        assert!(e.need.iter().any(|c| c.text.contains("尚在底线")), "{:?}", e.need);
        // 每条结论引用的证据都存在
        let ids: Vec<&String> = e.evidence.iter().map(|x| &x.id).collect();
        for c in e.need.iter().chain(&e.basis).chain(&e.comparison) {
            assert!(!c.evidence.is_empty());
            assert!(c.evidence.iter().all(|id| ids.contains(&id)));
        }
    }

    #[test]
    fn evidence_records_which_board_it_belongs_to() {
        // 炮五进四打中卒，对方马7进5吃炮：首选候选在落子前局面上，吃子与对方应对在落子后局面上
        let pos = parse_position("rnbakab1r/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR w").unwrap();
        let engine = EngineEvidence {
            candidates: vec![line("h0g2", Score::Cp(60), &["h0g2", "i9h9"]), line("b0c2", Score::Cp(50), &["b0c2"])],
            actual: Some(line("e2e6", Score::Cp(-320), &["e2e6", "g7e6", "h0g2"])),
            conditions: "测试".into(),
            complete: true,
            notes: vec![],
        };
        let e = explain_move(&pos, mv("e2e6"), &CTX, Some(&engine)).unwrap();
        let on = |topic: &str| e.evidence.iter().find(|x| x.topic == topic).and_then(|x| x.on);
        assert_eq!(on("best"), Some(LineBase::Parent));
        assert_eq!(on("quiet").or(on("threat")), Some(LineBase::Parent));
        assert_eq!(on("capture"), Some(LineBase::Node));
        assert_eq!(on("reply"), Some(LineBase::Node));
        // 说明一个局面时，证据都在这个局面上
        let p = explain_position(&pos, &CTX, Some(&engine));
        assert!(p.evidence.iter().all(|x| x.on == Some(LineBase::Node)), "{:?}", p.evidence);
    }

    #[test]
    fn evidence_saved_without_board_reads_as_unknown() {
        // 早期保存的证据没有 on：读取后为未知，不默认成落子后的局面
        let old = r#"{"id":"k1","kind":"variation","topic":"candidate","text":"首选","squares":["b0","c2"],
            "arrows":["b0c2"],"line":null,"certainty":"supported"}"#;
        let ev: Evidence = serde_json::from_str(old).unwrap();
        assert_eq!(ev.on, None);
        let new = serde_json::to_value(Evidence { on: Some(LineBase::Parent), ..ev }).unwrap();
        assert_eq!(new["on"], "parent");
    }

    #[test]
    fn mistake_follows_gain_cost_structure() {
        // 炮五进四打中卒，对方马7进5吃炮
        let pos = parse_position("rnbakab1r/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR w").unwrap();
        let engine = EngineEvidence {
            candidates: vec![
                line("h0g2", Score::Cp(60), &["h0g2", "i9h9"]),
                line("b0c2", Score::Cp(50), &["b0c2", "i9h9"]),
            ],
            actual: Some(line("e2e6", Score::Cp(-320), &["e2e6", "g7e6", "h0g2"])),
            conditions: "测试".into(),
            complete: true,
            notes: vec![],
        };
        let e = explain_move(&pos, mv("e2e6"), &CTX, Some(&engine)).unwrap();
        let v = e.verdict.clone().unwrap();
        assert_eq!(v.tone, VerdictTone::Blunder);
        assert_eq!(v.delta, Some(380));
        assert!(e.summary.starts_with("炮五进四：吃掉对方中卒，但"), "{}", e.summary);
        assert!(e.replies.iter().any(|c| c.text.contains("马7进5")), "{:?}", e.replies);
        assert!(e.replies.iter().any(|c| c.text.contains("净损失")), "{:?}", e.replies);
        assert!(e.comparison.iter().any(|c| c.text.contains("没有进入前 2 个候选")));
        assert!(e.lesson.contains("会不会被对方吃掉"));
        assert!(e.candidates.iter().any(|c| c.is_actual && c.source == CandidateSource::SearchMoves));
        assert!(e.warnings.is_empty(), "{:?}", e.warnings);
    }

    #[test]
    fn mate_and_bound_scores_are_not_subtracted() {
        let best = line("h0g2", Score::Mate(3), &["h0g2"]);
        let actual = line("b0c2", Score::Cp(500), &["b0c2"]);
        let v = judge(&best, &actual, Some(2));
        assert_eq!((v.tone, v.delta), (VerdictTone::Blunder, None));
        let mut bounded = line("b0c2", Score::Cp(10), &["b0c2"]);
        bounded.bound = Bound::Upper;
        let v = judge(&line("h0g2", Score::Cp(100), &["h0g2"]), &bounded, Some(2));
        assert_eq!(v.delta, None);
        assert!(!v.note.is_empty());
        let v = judge(&line("h0g2", Score::Cp(10), &["h0g2"]), &line("b0c2", Score::Cp(90), &["b0c2"]), None);
        assert_eq!(v.tone, VerdictTone::Unclear);
    }

    #[test]
    fn overlapping_bounds_do_not_produce_a_verdict() {
        // 评审中的用例：首选 +100 上界、实际 -200 下界，区间重叠，不能判为严重失误
        let mut best = line("h0g2", Score::Cp(100), &["h0g2"]);
        best.bound = Bound::Upper;
        let mut actual = line("b0c2", Score::Cp(-200), &["b0c2"]);
        actual.bound = Bound::Lower;
        let v = judge(&best, &actual, None);
        assert_eq!(v.tone, VerdictTone::Unclear, "{v:?}");
        // 区间能证明差距时才给出等级，但不显示精确分差
        let mut best = line("h0g2", Score::Cp(400), &["h0g2"]);
        best.bound = Bound::Lower;
        let mut actual = line("b0c2", Score::Cp(0), &["b0c2"]);
        actual.bound = Bound::Upper;
        let v = judge(&best, &actual, None);
        assert_eq!((v.tone, v.delta), (VerdictTone::Blunder, None));
        assert!(v.note.contains("至少"));
        // 有边界值的将杀分不下结论
        let mut mate = line("h0g2", Score::Mate(3), &["h0g2"]);
        mate.bound = Bound::Lower;
        assert_eq!(judge(&mate, &line("b0c2", Score::Cp(0), &["b0c2"]), Some(2)).tone, VerdictTone::Unclear);
    }

    #[test]
    fn incomplete_analysis_is_partial() {
        let pos = parse_position("rnbakab1r/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR w").unwrap();
        let candidates = vec![line("h0g2", Score::Cp(60), &["h0g2", "i9h9"]), line("b0c2", Score::Cp(50), &["b0c2"])];
        // 实际走法不在候选中，单独分析又失败
        let engine = EngineEvidence { candidates: candidates.clone(), actual: None, conditions: "测试".into(), complete: true, notes: vec![] };
        let e = explain_move(&pos, mv("e2e6"), &CTX, Some(&engine)).unwrap();
        assert_eq!(e.level, ExplainLevel::Partial);
        assert!(e.verdict.is_none());
        // 候选来自不完整的一轮搜索
        let engine = EngineEvidence {
            candidates,
            actual: None,
            conditions: "测试".into(),
            complete: false,
            notes: vec!["最后一轮搜索没有完成".into()],
        };
        let e = explain_move(&pos, mv("h0g2"), &CTX, Some(&engine)).unwrap();
        assert_eq!(e.level, ExplainLevel::Partial);
        assert!(e.warnings.iter().any(|w| w.contains("候选比较尚未完成")), "{:?}", e.warnings);
    }

    #[test]
    fn incomplete_candidates_give_no_verdict_or_best_move() {
        // 评审中的用例：深度 1 的候选为 [炮二平五 +400, 马二进三 -100]，但深度 2 的首位与最终着法都已变为马二进三。
        // 直接相减会把引擎最终选择的马二进三判为“严重失误”。
        let engine = EngineEvidence {
            candidates: vec![line("h2e2", Score::Cp(400), &["h2e2"]), line("h0g2", Score::Cp(-100), &["h0g2"])],
            actual: None,
            conditions: "测试".into(),
            complete: false,
            notes: vec!["引擎最终着法与候选首位不一致，比较结果可能不稳定".into()],
        };
        let e = explain_move(&Position::startpos(), mv("h0g2"), &CTX, Some(&engine)).unwrap();
        assert_eq!(e.level, ExplainLevel::Partial);
        let v = e.verdict.clone().unwrap();
        assert_eq!((v.tone, v.delta), (VerdictTone::Unclear, None));
        let mut texts = vec![e.summary.clone(), v.label.clone()];
        texts.extend(e.comparison.iter().chain(&e.replies).map(|c| c.text.clone()));
        texts.extend(e.evidence.iter().map(|x| x.text.clone()));
        for t in &texts {
            assert!(!["严重失误", "首选", "低约", "明显不如", "略逊"].iter().any(|w| t.contains(w)), "{t}");
        }
        assert!(e.comparison.iter().any(|c| c.text.contains("暂不比较")), "{:?}", e.comparison);
        // 局面分析同样不确定首选
        let p = explain_position(&Position::startpos(), &CTX, Some(&engine));
        assert!(p.claims.iter().all(|c| !c.text.contains("引擎首选")), "{:?}", p.claims);
        assert!(p.claims.iter().any(|c| c.text.contains("暂不确定首选")), "{:?}", p.claims);
        assert!(p.warnings.iter().any(|w| w.contains("不一致")), "{:?}", p.warnings);
    }

    #[test]
    fn bounded_mate_score_is_not_stated_as_mate() {
        let pos = parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 w").unwrap();
        let mut mate = line("e1d1", Score::Mate(3), &["e1d1"]);
        mate.bound = Bound::Upper;
        let engine = EngineEvidence {
            candidates: vec![mate.clone(), line("e1e2", Score::Cp(800), &["e1e2"])],
            actual: None,
            conditions: "测试".into(),
            complete: true,
            notes: vec![],
        };
        for played in ["e1d1", "e1e2"] {
            let e = explain_move(&pos, mv(played), &CTX, Some(&engine)).unwrap();
            let mut texts = vec![e.summary.clone()];
            texts.extend(e.comparison.iter().chain(&e.replies).map(|c| c.text.clone()));
            texts.extend(e.evidence.iter().map(|x| x.text.clone()));
            assert!(texts.iter().all(|t| !t.contains("步内杀棋")), "{played}: {texts:?}");
            assert!(texts.iter().any(|t| t.contains("将杀边界值")), "{played}: {texts:?}");
            if played == "e1e2" {
                assert_eq!(e.verdict.unwrap().tone, VerdictTone::Unclear);
            }
        }
        // 精确的将杀分仍给出步数
        let exact = EngineEvidence { candidates: vec![line("e1d1", Score::Mate(1), &["e1d1"])], ..engine };
        let e = explain_move(&pos, mv("e1d1"), &CTX, Some(&exact)).unwrap();
        assert!(e.replies.iter().any(|c| c.text.contains("1 步内杀棋")), "{:?}", e.replies);
        let p = explain_position(&pos, &CTX, Some(&exact));
        assert!(p.claims.iter().any(|c| c.text.contains("可在 1 步内杀棋")), "{:?}", p.claims);
    }

    #[test]
    fn false_cannon_protection_is_not_explained() {
        let pos = parse_position("3k5/9/9/9/9/C8/2r1N4/9/9/4K4 w").unwrap();
        let e = explain_move(&pos, mv("a4a3"), &CTX, None).unwrap();
        assert!(e.basis.iter().all(|c| !c.text.contains("在新位置保护")), "{:?}", e.basis);
        // 真实的保护写明对应的吃子与回吃
        let pos = parse_position("4rk3/9/9/9/4N4/R8/9/9/9/3K5 w").unwrap();
        let e = explain_move(&pos, mv("a4a5"), &CTX, None).unwrap();
        assert!(e.basis.iter().any(|c| c.text.contains("对方5路车吃下后，九路车可以吃回")), "{:?}", e.basis);
    }

    #[test]
    fn check_escape_by_removing_screen_is_explained() {
        let pos = parse_position("3k5/9/4c4/9/9/9/9/4N4/9/4K4 w").unwrap();
        let e = explain_move(&pos, mv("e2c3"), &CTX, None).unwrap();
        assert!(e.basis.iter().any(|c| c.text.contains("唯一的炮架")), "{:?}", e.basis);
        assert!(!e.basis.iter().any(|c| c.text.contains("挡在")), "{:?}", e.basis);
    }

    #[test]
    fn invalid_engine_lines_are_dropped() {
        let engine = EngineEvidence {
            candidates: vec![line("h0g2", Score::Cp(20), &["h0g2", "h0g2"])],
            actual: None,
            conditions: "测试".into(),
            complete: true,
            notes: vec![],
        };
        let e = explain_move(&Position::startpos(), mv("h0g2"), &CTX, Some(&engine)).unwrap();
        assert!(e.warnings.iter().any(|w| w.contains("无法在当前局面合法回放")), "{:?}", e.warnings);
        assert!(e.evidence.iter().all(|x| x.line.as_ref().is_none_or(|l| l.moves.len() < 2 || l.moves[1] != mv("h0g2"))));
        // 候选同样移除这条变化，界面与其他展示路径都不能再回放它
        assert!(e.candidates[0].pv.is_empty() && e.candidates[0].pv_notation.is_empty(), "{:?}", e.candidates[0]);
        assert!(e.warnings.iter().any(|w| w.contains("候选马二进三的主变化")), "{:?}", e.warnings);
        let p = explain_position(&Position::startpos(), &CTX, Some(&engine));
        assert!(p.candidates[0].pv.is_empty());
        assert!(p.evidence.iter().all(|x| x.line.is_none()), "{:?}", p.evidence);
    }

    #[test]
    fn endgame_position_and_terminal_explanations() {
        let ctx = ExplainContext { history_complete: false, repetition_count: 1 };
        let pos = parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 b").unwrap();
        let p = explain_position(&pos, &ctx, None);
        assert!(p.summary.starts_with("轮到黑方走"), "{}", p.summary);
        assert!(p.warnings.iter().any(|w| w.contains("着法未知")));
        let stale = explain_position(&parse_position("3k5/4R4/4R4/9/9/9/9/9/9/4K4 b").unwrap(), &ctx, None);
        assert_eq!(stale.status, GameStatus::Stalemate);
        assert!(stale.claims[0].text.contains("可走点都不安全"), "{:?}", stale.claims);
        let mated = explain_position(&parse_position("3k5/3R5/3R5/9/9/9/9/9/9/4K4 b").unwrap(), &ctx, None);
        assert_eq!(mated.status, GameStatus::Checkmate);
        // 走出将死的一着
        let pos = parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 w").unwrap();
        let e = explain_move(&pos, mv("e1d1"), &ctx, None).unwrap();
        assert!(e.summary.contains("将死对方"), "{}", e.summary);
    }

    #[test]
    fn check_escape_need_and_lesson() {
        let pos = parse_position("3k5/9/9/9/9/9/9/9/3R5/5K3 b").unwrap();
        let e = explain_move(&pos, mv("d9e9"), &CTX, None).unwrap();
        assert!(e.need[0].text.contains("正被将军"));
        assert!(e.basis.iter().any(|c| c.text.contains("躲开")), "{:?}", e.basis);
        assert!(e.lesson.contains("应将"));
    }
}
