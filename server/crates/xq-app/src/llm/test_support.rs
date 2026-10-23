//! 单元测试共用：构造学习会话、讲解记录与证据包。

use xq_core::Move;
use xq_core::Position;
use xq_core::explain::EngineEvidence;
use xq_core::explain::EngineLine;
use xq_core::explain::ExplainContext;
use xq_core::fen::parse_position;
use xq_core::study::MoveSource;
use xq_core::study::NewStudy;
use xq_core::study::NodeId;
use xq_core::study::Orientation;
use xq_core::study::ROOT;
use xq_core::study::RuleSet;
use xq_core::study::Study;
use xq_core::study::StudySource;
use xq_core::uci::Bound;
use xq_core::uci::Score;

use super::evidence::EvidenceBundle;
use super::evidence::build;
use crate::records::ExplanationRecord;

pub fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

pub fn study_from(start: Position, moves: &[&str]) -> Study {
    let standard = start.is_startpos();
    let mut study = Study::new(NewStudy {
        id: "s".into(),
        name: "测试".into(),
        source: if standard { StudySource::Standard } else { StudySource::Fen },
        start,
        counters: None,
        history_complete: standard,
        orientation: Orientation::RedBottom,
        user_side: xq_core::Color::Red,
        rules: RuleSet::default(),
        now: 0,
    })
    .unwrap();
    let mut cur = ROOT;
    for m in moves {
        cur = study.apply_move(cur, mv(m), MoveSource::User, 0).unwrap().node;
    }
    study
}

pub fn study_with(moves: &[&str]) -> Study { study_from(Position::startpos(), moves) }

pub fn record(study: &Study, node: NodeId, engine: Option<&EngineEvidence>) -> ExplanationRecord {
    let n = study.node(node).unwrap();
    let before = study.position(n.parent.unwrap()).unwrap();
    let ctx = ExplainContext { history_complete: study.history_complete, repetition_count: 1 };
    let mut r = ExplanationRecord::new(&study.id, node, 0);
    r.explanation = xq_core::explain::explain_move(&before, n.mv.unwrap(), &ctx, engine);
    r
}

pub fn line(m: &str, cp: i32, pv: &[&str]) -> EngineLine {
    EngineLine { mv: mv(m), score: Score::Cp(cp), bound: Bound::Exact, depth: 10, pv: pv.iter().map(|s| mv(s)).collect() }
}

/// 起始局面走马二进三，给定候选与完整性。
pub fn detailed_bundle(lines: &[(&str, i32, &[&str])], complete: bool) -> EvidenceBundle {
    let study = study_with(&["h0g2"]);
    let engine = EngineEvidence {
        candidates: lines.iter().map(|(m, cp, pv)| line(m, *cp, pv)).collect(),
        actual: None,
        conditions: "测试引擎 · 快速分析（≤1.0 秒，深度≤10）· 2 个候选".into(),
        complete,
        notes: if complete { vec![] } else { vec!["引擎最终着法与候选首位不一致，比较结果可能不稳定".into()] },
    };
    build(&study, 1, &record(&study, 1, Some(&engine))).unwrap()
}

/// 炮五进四打中卒，会被马吃掉：程序评价为严重失误。
pub fn blunder_bundle() -> EvidenceBundle {
    let start = parse_position("rnbakab1r/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR w").unwrap();
    let study = study_from(start, &["e2e6"]);
    let engine = EngineEvidence {
        candidates: vec![line("h0g2", 60, &["h0g2", "i9h9"]), line("b0c2", 50, &["b0c2", "i9h9"])],
        actual: Some(line("e2e6", -320, &["e2e6", "g7e6", "h0g2"])),
        conditions: "测试".into(),
        complete: true,
        notes: vec![],
    };
    build(&study, 1, &record(&study, 1, Some(&engine))).unwrap()
}

/// 起始局面走马二进三，主变化给出精确的将杀分；`complete` 决定分析是否完整。
pub fn mate_bundle(complete: bool) -> EvidenceBundle {
    let study = study_with(&["h0g2"]);
    let mut mate = line("h0g2", 0, &["h0g2", "h9g7"]);
    mate.score = Score::Mate(3);
    let engine = EngineEvidence {
        candidates: vec![mate, line("b0c2", 20, &["b0c2"])],
        actual: None,
        conditions: "测试".into(),
        complete,
        notes: if complete { vec![] } else { vec!["最后一轮搜索没有完成".into()] },
    };
    build(&study, 1, &record(&study, 1, Some(&engine))).unwrap()
}
