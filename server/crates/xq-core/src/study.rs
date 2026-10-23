//! 学习会话的棋谱树。
//!
//! 每个节点保存落子后的局面；走子节点同时记录走法和中文记谱，落子前局面就是父节点局面。
//! 第一个子节点是主线，其余为分支。识别无法补全中间走法时，用“缺口节点”开始新的一段：
//! 它没有走法，此前历史视为未知。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use crate::board::Position;
use crate::fen;
use crate::notation;
use crate::rules;
use crate::rules::IllegalMove;
use crate::types::Color;
use crate::types::Move;
use crate::validate;
use crate::validate::ValidationReport;

pub type NodeId = u32;
pub const ROOT: NodeId = 0;
pub const STUDY_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudySource {
    Standard,
    Setup,
    Fen,
    Record,
    Capture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveSource {
    User,
    Record,
    Capture,
    Variation,
}

/// 棋盘显示方向，只影响显示与截图坐标换算，不影响行棋方。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    RedBottom,
    BlackBottom,
}

/// 与引擎一致的规则配置，随分析结果一起记录。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSet {
    pub repetition_rule: String,
    pub sixty_move_rule: bool,
}

impl Default for RuleSet {
    fn default() -> Self { Self { repetition_rule: "ChineseRule".to_string(), sixty_move_rule: true } }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub mv: Option<Move>,
    pub notation: Option<String>,
    /// 落子后（或缺口节点的）完整局面 FEN。
    pub fen: String,
    pub side_to_move: Color,
    pub ply: u32,
    pub source: MoveSource,
    #[serde(default)]
    pub gap: bool,
    #[serde(default)]
    pub gap_reason: Option<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub starred: bool,
    pub created_at: i64,
}

impl Node {
    pub fn is_move(&self) -> bool { self.mv.is_some() }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Study {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub source: StudySource,
    pub root_fen: String,
    /// 根局面之前的历史是否完整（标准开局为完整；残局、截图起点为不完整）。
    pub history_complete: bool,
    pub orientation: Orientation,
    pub user_side: Color,
    pub rules: RuleSet,
    pub nodes: BTreeMap<NodeId, Node>,
    pub next_id: NodeId,
    pub revision: u64,
    /// 屏幕连接时实际对局的最新节点。
    #[serde(default)]
    pub live_node: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StudyError {
    InvalidPosition(ValidationReport),
    NoSuchNode(NodeId),
    Illegal(IllegalMove),
    CannotDeleteRoot,
}

impl std::fmt::Display for StudyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StudyError::InvalidPosition(r) => {
                let reasons: Vec<&str> = r.issues.iter().map(|i| i.message.as_str()).collect();
                write!(f, "局面不成立：{}", reasons.join("；"))
            }
            StudyError::NoSuchNode(id) => write!(f, "棋谱中没有节点 {id}"),
            StudyError::Illegal(i) => write!(f, "{}", i.message),
            StudyError::CannotDeleteRoot => write!(f, "不能删除起始局面"),
        }
    }
}

impl std::error::Error for StudyError {}

pub struct NewStudy {
    pub id: String,
    pub name: String,
    pub source: StudySource,
    pub start: Position,
    /// 导入的 FEN 中给出的计数字段，保存在根节点 FEN 中并传给引擎。
    pub counters: Option<(u32, u32)>,
    pub history_complete: bool,
    pub orientation: Orientation,
    pub user_side: Color,
    pub rules: RuleSet,
    pub now: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveOutcome {
    pub node: NodeId,
    pub created: bool,
}

impl Study {
    pub fn new(spec: NewStudy) -> Result<Study, StudyError> {
        let report = validate::validate_position(&spec.start);
        if !report.ok {
            return Err(StudyError::InvalidPosition(report));
        }
        let root_fen = fen::position_fen_with_counters(&spec.start, spec.counters);
        let root = Node {
            id: ROOT,
            parent: None,
            children: Vec::new(),
            mv: None,
            notation: None,
            fen: root_fen.clone(),
            side_to_move: spec.start.side,
            ply: 0,
            source: MoveSource::User,
            gap: false,
            gap_reason: None,
            note: String::new(),
            starred: false,
            created_at: spec.now,
        };
        Ok(Study {
            format_version: STUDY_FORMAT_VERSION,
            id: spec.id,
            name: spec.name,
            created_at: spec.now,
            updated_at: spec.now,
            source: spec.source,
            history_complete: spec.history_complete && spec.start.is_startpos(),
            root_fen,
            orientation: spec.orientation,
            user_side: spec.user_side,
            rules: spec.rules,
            nodes: BTreeMap::from([(ROOT, root)]),
            next_id: 1,
            revision: 1,
            live_node: None,
        })
    }

    pub fn node(&self, id: NodeId) -> Result<&Node, StudyError> { self.nodes.get(&id).ok_or(StudyError::NoSuchNode(id)) }

    /// 节点局面。FEN 由本模块生成，解析失败说明数据损坏。
    pub fn position(&self, id: NodeId) -> Result<Position, StudyError> {
        let node = self.node(id)?;
        fen::parse_position(&node.fen).map_err(|_| StudyError::NoSuchNode(id))
    }

    fn touch(&mut self, now: i64) {
        self.revision += 1;
        self.updated_at = now;
    }

    /// 在 `parent` 后走一步。已有相同走法的子节点时直接返回它。
    pub fn apply_move(
        &mut self, parent: NodeId, mv: Move, source: MoveSource, now: i64,
    ) -> Result<MoveOutcome, StudyError> {
        let pos = self.position(parent)?;
        if let Some(existing) =
            self.node(parent)?.children.iter().find(|c| self.nodes.get(c).and_then(|n| n.mv) == Some(mv))
        {
            return Ok(MoveOutcome { node: *existing, created: false });
        }
        if let Some(reason) = rules::illegal_reason(&pos, mv) {
            return Err(StudyError::Illegal(reason));
        }
        let notation = notation::move_to_chinese(&pos, mv).ok();
        let after = pos.after(mv);
        let id = self.next_id;
        self.next_id += 1;
        let ply = self.node(parent)?.ply + 1;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                children: Vec::new(),
                mv: Some(mv),
                notation,
                fen: fen::position_fen(&after),
                side_to_move: after.side,
                ply,
                source,
                gap: false,
                gap_reason: None,
                note: String::new(),
                starred: false,
                created_at: now,
            },
        );
        self.nodes.get_mut(&parent).expect("父节点已检查").children.push(id);
        self.touch(now);
        Ok(MoveOutcome { node: id, created: true })
    }

    /// 在 `parent` 后建立缺口节点：画面变化无法用合法走法解释时，从新局面重新开始一段。
    pub fn add_gap(&mut self, parent: NodeId, pos: &Position, reason: &str, now: i64) -> Result<NodeId, StudyError> {
        let report = validate::validate_position(pos);
        if !report.ok {
            return Err(StudyError::InvalidPosition(report));
        }
        let ply = self.node(parent)?.ply;
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                children: Vec::new(),
                mv: None,
                notation: None,
                fen: fen::position_fen(pos),
                side_to_move: pos.side,
                ply,
                source: MoveSource::Capture,
                gap: true,
                gap_reason: Some(reason.to_string()),
                note: String::new(),
                starred: false,
                created_at: now,
            },
        );
        self.nodes.get_mut(&parent).expect("父节点已检查").children.push(id);
        self.touch(now);
        Ok(id)
    }

    /// 从根到该节点的路径（含两端）。
    pub fn path(&self, id: NodeId) -> Result<Vec<NodeId>, StudyError> {
        let mut path = vec![id];
        let mut cur = self.node(id)?;
        while let Some(parent) = cur.parent {
            path.push(parent);
            cur = self.node(parent)?;
        }
        path.reverse();
        Ok(path)
    }

    /// 该节点所在段的起点：根或最近的缺口节点。
    pub fn segment_start(&self, id: NodeId) -> Result<NodeId, StudyError> {
        let path = self.path(id)?;
        Ok(path.iter().rev().copied().find(|n| *n == ROOT || self.nodes.get(n).is_some_and(|x| x.gap)).unwrap_or(ROOT))
    }

    /// 发给引擎的起始 FEN 与此后走法，用于重复局面等依赖历史的判断。
    pub fn engine_history(&self, id: NodeId) -> Result<(String, Vec<Move>), StudyError> {
        let start = self.segment_start(id)?;
        let path = self.path(id)?;
        let from = path.iter().position(|n| *n == start).unwrap_or(0);
        let moves = path[from + 1..].iter().filter_map(|n| self.nodes.get(n).and_then(|x| x.mv)).collect();
        Ok((self.node(start)?.fen.clone(), moves))
    }

    /// 到该节点为止历史是否完整（影响重复局面判定的可信程度）。
    pub fn history_complete_at(&self, id: NodeId) -> Result<bool, StudyError> {
        Ok(self.history_complete && self.segment_start(id)? == ROOT)
    }

    /// 当前局面在本段历史中出现的次数（含自身）。
    pub fn repetition_count(&self, id: NodeId) -> Result<usize, StudyError> {
        let start = self.segment_start(id)?;
        let path = self.path(id)?;
        // 根节点 FEN 可能带计数字段，只比较布局与行棋方
        let target = fen::position_key(&self.node(id)?.fen);
        let from = path.iter().position(|n| *n == start).unwrap_or(0);
        Ok(path[from..].iter().filter(|n| self.nodes.get(n).is_some_and(|x| fen::position_key(&x.fen) == target)).count())
    }

    pub fn mainline(&self) -> Vec<NodeId> {
        let mut line = vec![ROOT];
        let mut cur = ROOT;
        while let Some(next) = self.nodes.get(&cur).and_then(|n| n.children.first().copied()) {
            line.push(next);
            cur = next;
        }
        line
    }

    /// 把节点所在分支提升为父节点的主线。
    pub fn promote(&mut self, id: NodeId, now: i64) -> Result<(), StudyError> {
        let parent = self.node(id)?.parent.ok_or(StudyError::CannotDeleteRoot)?;
        let siblings = &mut self.nodes.get_mut(&parent).ok_or(StudyError::NoSuchNode(parent))?.children;
        siblings.retain(|c| *c != id);
        siblings.insert(0, id);
        self.touch(now);
        Ok(())
    }

    /// 删除节点及其后续，返回被删除的节点编号。
    pub fn delete_subtree(&mut self, id: NodeId, now: i64) -> Result<Vec<NodeId>, StudyError> {
        let parent = self.node(id)?.parent.ok_or(StudyError::CannotDeleteRoot)?;
        let mut removed = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(node) = self.nodes.remove(&n) {
                stack.extend(node.children);
                removed.push(n);
            }
        }
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.retain(|c| *c != id);
        }
        if self.live_node.is_some_and(|l| removed.contains(&l)) {
            self.live_node = Some(parent);
        }
        self.touch(now);
        Ok(removed)
    }

    pub fn set_note(&mut self, id: NodeId, note: String, starred: bool, now: i64) -> Result<(), StudyError> {
        let node = self.nodes.get_mut(&id).ok_or(StudyError::NoSuchNode(id))?;
        node.note = note;
        node.starred = starred;
        self.touch(now);
        Ok(())
    }

    pub fn set_live(&mut self, id: Option<NodeId>, now: i64) {
        self.live_node = id;
        self.touch(now);
    }

    pub fn move_nodes(&self) -> impl Iterator<Item = &Node> { self.nodes.values().filter(|n| n.is_move()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn study(start: Position) -> Study {
        Study::new(NewStudy {
            id: "s1".into(),
            name: "测试".into(),
            source: StudySource::Fen,
            start,
            counters: None,
            history_complete: true,
            orientation: Orientation::RedBottom,
            user_side: Color::Red,
            rules: RuleSet::default(),
            now: 0,
        })
        .unwrap()
    }

    fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

    #[test]
    fn branches_and_mainline() {
        let mut s = study(Position::startpos());
        let a = s.apply_move(ROOT, mv("h2e2"), MoveSource::User, 1).unwrap();
        assert!(a.created);
        let again = s.apply_move(ROOT, mv("h2e2"), MoveSource::User, 2).unwrap();
        assert_eq!(again, MoveOutcome { node: a.node, created: false });
        let b = s.apply_move(ROOT, mv("h0g2"), MoveSource::Variation, 3).unwrap();
        assert_eq!(s.node(ROOT).unwrap().children, vec![a.node, b.node]);
        assert_eq!(s.mainline(), vec![ROOT, a.node]);
        s.promote(b.node, 4).unwrap();
        assert_eq!(s.mainline(), vec![ROOT, b.node]);
        assert_eq!(s.node(b.node).unwrap().notation.as_deref(), Some("马二进三"));
        let removed = s.delete_subtree(a.node, 5).unwrap();
        assert_eq!(removed, vec![a.node]);
        assert!(matches!(s.delete_subtree(ROOT, 6), Err(StudyError::CannotDeleteRoot)));
    }

    #[test]
    fn illegal_moves_are_explained() {
        let mut s = study(Position::startpos());
        match s.apply_move(ROOT, mv("h0f1"), MoveSource::User, 1) {
            Err(StudyError::Illegal(i)) => assert_eq!(i.code, "horse_leg"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn endgame_start_keeps_side_and_marks_history() {
        let start = fen::parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 b").unwrap();
        let s = study(start);
        assert_eq!(s.node(ROOT).unwrap().side_to_move, Color::Black);
        assert!(!s.history_complete, "残局起点的历史不完整");
    }

    #[test]
    fn rejects_invalid_start() {
        let bad = fen::parse_position("4k4/9/9/9/9/9/9/9/9/4K4 w").unwrap();
        let result = Study::new(NewStudy {
            id: "x".into(),
            name: "x".into(),
            source: StudySource::Fen,
            start: bad,
            counters: None,
            history_complete: false,
            orientation: Orientation::RedBottom,
            user_side: Color::Red,
            rules: RuleSet::default(),
            now: 0,
        });
        assert!(matches!(result, Err(StudyError::InvalidPosition(_))));
    }

    #[test]
    fn root_counters_are_kept_for_engine() {
        let parsed = fen::parse_fen("3k5/9/9/9/9/9/9/9/4R4/4K4 w - - 30 55").unwrap();
        let start = Position::new(parsed.board, parsed.side.unwrap());
        let mut s = Study::new(NewStudy {
            id: "c".into(),
            name: "计数".into(),
            source: StudySource::Fen,
            start,
            counters: parsed.counters,
            history_complete: false,
            orientation: Orientation::RedBottom,
            user_side: Color::Red,
            rules: RuleSet::default(),
            now: 0,
        })
        .unwrap();
        let mut cur = ROOT;
        for m in ["e1e2", "d9d8", "e2e1", "d8d9"] {
            cur = s.apply_move(cur, mv(m), MoveSource::User, 1).unwrap().node;
        }
        let (base, moves) = s.engine_history(cur).unwrap();
        assert_eq!(base, "3k5/9/9/9/9/9/9/9/4R4/4K4 w - - 30 55");
        assert_eq!(moves.len(), 4);
        assert_eq!(s.repetition_count(cur).unwrap(), 2, "根节点带计数字段时仍能识别重复局面");
    }

    #[test]
    fn history_and_repetition_across_gap() {
        let mut s = study(Position::startpos());
        let mut cur = ROOT;
        for m in ["h0g2", "h9g7", "g2h0", "g7h9"] {
            cur = s.apply_move(cur, mv(m), MoveSource::User, 1).unwrap().node;
        }
        assert_eq!(s.repetition_count(cur).unwrap(), 2);
        let (base, moves) = s.engine_history(cur).unwrap();
        assert_eq!(base, fen::START_FEN);
        assert_eq!(moves.len(), 4);
        let gap_pos = fen::parse_position("3k5/9/9/9/9/9/9/9/4R4/4K4 b").unwrap();
        let gap = s.add_gap(cur, &gap_pos, "漏帧", 2).unwrap();
        let next = s.apply_move(gap, mv("d9d8"), MoveSource::Capture, 3).unwrap().node;
        let (base, moves) = s.engine_history(next).unwrap();
        assert_eq!(base, fen::position_fen(&gap_pos));
        assert_eq!(moves, vec![mv("d9d8")]);
        assert!(!s.history_complete_at(next).unwrap());
        assert!(s.history_complete_at(cur).unwrap());
    }
}
