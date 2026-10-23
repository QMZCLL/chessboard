//! 证据包：交给语言模型的局面、规则事实、引擎结果与证据质量。
//!
//! 内容全部取自已经生成并检查过的逐着讲解，模型只能在“允许表达的结论”（`claims`）范围内组织语言。
//! 证据版本是整个证据包的指纹，覆盖实际使用的搜索结果：同一条件下重新搜索，
//! 只要候选、评分或完整性有变化，版本就会改变，旧的 AI 讲解随之过期。

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use xq_core::explain::Certainty;
use xq_core::explain::Evidence;
use xq_core::explain::EvidenceKind;
use xq_core::explain::ExplainLevel;
use xq_core::explain::LineBase;
use xq_core::explain::LineRef;
use xq_core::explain::VerdictTone;
use xq_core::explain::score_label;
use xq_core::study::NodeId;
use xq_core::study::Study;

use crate::records::ExplanationRecord;

/// 证据包结构版本；字段或生成方式改变时递增。
pub const PACK_VERSION: u32 = 1;
/// 证据包中最多列出的历史着法数（更早的只计数）。
const HISTORY_LIMIT: usize = 60;

/// 允许模型表达的一条结论。由程序从讲解中已有的结论生成，模型可以选择和衔接，但不能新增。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowedClaim {
    pub id: String,
    /// 结论类别，取自证据主题，如 check、escape、capture、protect、best、reply、verdict、candidate、lesson。
    pub kind: String,
    /// 来源：need、basis、comparison、reply、verdict、candidate、lesson。
    pub group: String,
    pub statement: String,
    pub evidence_ids: Vec<String>,
    /// certain 为规则事实，supported 为当前搜索支持的判断，tentative 为位置性解释或尚未确认的内容。
    pub certainty: Certainty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackHistory {
    pub root_fen: String,
    /// 从根局面到落子前的着法（中文记谱），局面不连续处记为“（局面不连续）”。
    pub moves: Vec<String>,
    /// 因篇幅省略的更早着法数。
    pub omitted: usize,
    /// 此前历史是否完整（残局、截图起点或局面不连续时为否）。
    pub complete: bool,
    /// 落子后的局面在本段棋谱中出现的次数。
    pub repetition_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackRules {
    pub repetition_rule: String,
    pub sixty_move_rule: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackAnalysis {
    /// basic：只有规则事实；partial：引擎分析不完整；detailed：候选与实际着法都已分析。
    pub level: String,
    pub complete: bool,
    pub conditions: Option<String>,
    pub error: Option<String>,
    /// 按证据质量允许表达的范围。
    pub allowed_scope: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackVerdict {
    pub tone: VerdictTone,
    pub label: String,
    pub delta: Option<i32>,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackCandidate {
    /// 多候选搜索中的名次；实际着法单独分析时为空。
    pub rank: Option<u32>,
    pub notation: String,
    pub is_actual: bool,
    /// 程序给出的评分说明（已注明边界值）。
    pub score: String,
    pub depth: u32,
    /// 主变化（中文记谱），从落子前局面开始。
    pub line: Vec<String>,
    /// 首着的直接作用（规则事实）。
    pub effect: String,
    pub claim_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoachEvidencePack {
    pub pack_version: u32,
    pub generator_version: u32,
    pub mover: String,
    pub move_notation: String,
    pub move_iccs: String,
    pub fen_before: String,
    pub fen_after: String,
    pub score_perspective: String,
    pub line_start: String,
    pub history: PackHistory,
    pub rules: PackRules,
    pub analysis: PackAnalysis,
    /// 程序给出的评价（等级、分差由程序控制，界面单独显示）。
    pub verdict: Option<PackVerdict>,
    pub claims: Vec<AllowedClaim>,
    pub candidates: Vec<PackCandidate>,
    pub warnings: Vec<String>,
}

/// 证据包及后台保留的对应关系（证据列表不发给模型，用于界面高亮与回放）。
#[derive(Clone, Debug)]
pub struct EvidenceBundle {
    pub study_id: String,
    pub node_id: NodeId,
    pub pack: CoachEvidencePack,
    /// 证据版本：证据包内容的指纹。
    pub revision: String,
    /// 讲解中的证据，加上为每个候选补充的回放证据。
    pub evidence: Vec<Evidence>,
}

impl EvidenceBundle {
    pub fn claim(&self, id: &str) -> Option<&AllowedClaim> { self.pack.claims.iter().find(|c| c.id == id) }
}

fn strength(c: Certainty) -> u8 {
    match c {
        Certainty::Tentative => 0,
        Certainty::Supported => 1,
        Certainty::Certain => 2,
    }
}

fn weakest(evidence: &[Evidence], ids: &[String]) -> Certainty {
    ids.iter()
        .filter_map(|id| evidence.iter().find(|e| &e.id == id))
        .map(|e| e.certainty)
        .min_by_key(|c| strength(*c))
        .unwrap_or(Certainty::Tentative)
}

fn allowed_scope(level: ExplainLevel, history_complete: bool) -> Vec<String> {
    let mut scope = vec![
        "只能使用 claims 中的结论；不得新增输入中没有的走法、变化、评分、胜率或将杀步数".to_string(),
        "评分只代表当前搜索条件下的评估，不能换算成胜率".to_string(),
    ];
    scope.push(
        match level {
            ExplainLevel::Basic => "没有引擎分析结果：只说明规则事实与位置性判断，不比较候选优劣",
            ExplainLevel::Partial => {
                "分析尚未完成：不得评价优劣（不得使用首选、失误、更好、更差等说法），候选与变化只作参考"
            }
            ExplainLevel::Detailed => "可以按 verdict 说明这步与首选的关系；等级与分差由程序给出，不得改写",
        }
        .to_string(),
    );
    if !history_complete {
        scope.push("此前的着法历史不完整：不得断言长将、长捉或重复局面的判罚".to_string());
    }
    scope
}

/// 由学习会话与该着的讲解记录生成证据包。
pub fn build(study: &Study, node_id: NodeId, record: &ExplanationRecord) -> Result<EvidenceBundle, String> {
    let node = study.node(node_id).map_err(|e| e.to_string())?;
    let (Some(parent), Some(mv)) = (node.parent, node.mv) else { return Err("只有走子节点才有逐着讲解".into()) };
    let expl = record.explanation.as_ref().ok_or("这一着还没有讲解")?;
    let parent_node = study.node(parent).map_err(|e| e.to_string())?;

    let mut moves = Vec::new();
    for id in study.path(parent).map_err(|e| e.to_string())?.into_iter().skip(1) {
        let n = study.node(id).map_err(|e| e.to_string())?;
        if n.gap {
            moves.push("（局面不连续）".to_string());
        } else if let Some(text) = &n.notation {
            moves.push(text.clone());
        }
    }
    let omitted = moves.len().saturating_sub(HISTORY_LIMIT);
    let moves = moves.split_off(omitted);
    let history_complete = study.history_complete_at(parent).map_err(|e| e.to_string())?;
    let complete = expl.level == ExplainLevel::Detailed;

    let mut evidence = expl.evidence.clone();
    let mut claims = Vec::new();
    let mut add = |kind: String, group: &str, statement: String, ids: Vec<String>, certainty: Certainty| {
        let id = format!("c{}", claims.len() + 1);
        claims.push(AllowedClaim { id: id.clone(), kind, group: group.to_string(), statement, evidence_ids: ids, certainty });
        id
    };
    for (group, list) in [("need", &expl.need), ("basis", &expl.basis), ("comparison", &expl.comparison), ("reply", &expl.replies)] {
        for c in list {
            let kind = c
                .evidence
                .first()
                .and_then(|id| expl.evidence.iter().find(|e| &e.id == id))
                .map(|e| e.topic.clone())
                .unwrap_or_else(|| "statement".to_string());
            add(kind, group, c.text.clone(), c.evidence.clone(), weakest(&expl.evidence, &c.evidence));
        }
    }
    if let Some(v) = &expl.verdict {
        let mut statement = match v.delta {
            Some(d) => format!("评价：{}（比首选低约 {d} 分）", v.label),
            None => format!("评价：{}", v.label),
        };
        if !v.note.is_empty() {
            statement.push_str(&format!("；{}", v.note));
        }
        let ids: Vec<String> = expl.comparison.iter().flat_map(|c| c.evidence.iter().cloned()).collect();
        let certainty = if v.tone == VerdictTone::Unclear { Certainty::Tentative } else { Certainty::Supported };
        add("verdict".into(), "verdict", statement, ids, certainty);
    }
    add("lesson".into(), "lesson", expl.lesson.clone(), Vec::new(), Certainty::Tentative);

    // 候选：补充可回放的证据，并作为可引用的结论
    let candidate_certainty = if complete { Certainty::Supported } else { Certainty::Tentative };
    let mut candidates = Vec::new();
    for (i, c) in expl.candidates.iter().enumerate() {
        let score = score_label(c.score, c.bound, expl.mover);
        let evidence_id = format!("k{}", i + 1);
        let prefix = match c.rank {
            Some(r) => format!("候选第 {r} 名"),
            None => "实际着法单独分析".to_string(),
        };
        let mut statement = format!("{prefix}{}（{score}，深度 {}）", c.notation, c.depth);
        if c.is_actual && c.rank.is_some() {
            statement.push_str("，即实际着法");
        }
        if !c.pv_notation.is_empty() {
            statement.push_str(&format!("；主变化 {}", c.pv_notation.join(" ")));
        }
        if !c.summary.is_empty() {
            statement.push_str(&format!("；首着作用：{}", c.summary));
        }
        if !complete {
            statement.push_str("（分析尚未完成，排名与评分只作参考）");
        }
        evidence.push(Evidence {
            id: evidence_id.clone(),
            kind: EvidenceKind::Variation,
            topic: "candidate".into(),
            text: statement.clone(),
            squares: vec![c.mv.from, c.mv.to],
            arrows: vec![c.mv],
            line: (!c.pv.is_empty()).then(|| LineRef {
                base: LineBase::Parent,
                moves: c.pv.clone(),
                notation: c.pv_notation.clone(),
            }),
            certainty: candidate_certainty,
            on: Some(LineBase::Parent),
        });
        let claim_id = add("candidate".into(), "candidate", statement, vec![evidence_id], candidate_certainty);
        candidates.push(PackCandidate {
            rank: c.rank,
            notation: c.notation.clone(),
            is_actual: c.is_actual,
            score,
            depth: c.depth,
            line: c.pv_notation.clone(),
            effect: c.summary.clone(),
            claim_id,
        });
    }

    let pack = CoachEvidencePack {
        pack_version: PACK_VERSION,
        generator_version: expl.generator_version,
        mover: expl.mover.label().to_string(),
        move_notation: expl.notation.clone(),
        move_iccs: mv.iccs(),
        fen_before: parent_node.fen.clone(),
        fen_after: node.fen.clone(),
        score_perspective: format!("所有评分都以落子前的走棋方（{}）为视角，正数对{}有利", expl.mover.label(), expl.mover.label()),
        line_start: "候选与主变化都从落子前的局面开始".to_string(),
        history: PackHistory {
            root_fen: study.root_fen.clone(),
            moves,
            omitted,
            complete: history_complete,
            repetition_count: study.repetition_count(node_id).map_err(|e| e.to_string())?,
        },
        rules: PackRules {
            repetition_rule: study.rules.repetition_rule.clone(),
            sixty_move_rule: study.rules.sixty_move_rule,
        },
        analysis: PackAnalysis {
            level: match expl.level {
                ExplainLevel::Basic => "basic",
                ExplainLevel::Partial => "partial",
                ExplainLevel::Detailed => "detailed",
            }
            .to_string(),
            complete,
            conditions: expl.conditions.clone(),
            error: record.error.clone(),
            allowed_scope: allowed_scope(expl.level, history_complete),
        },
        verdict: expl.verdict.as_ref().map(|v| PackVerdict {
            tone: v.tone,
            label: v.label.clone(),
            delta: v.delta,
            note: v.note.clone(),
        }),
        claims,
        candidates,
        warnings: expl.warnings.clone(),
    };
    let revision = fingerprint(&pack);
    Ok(EvidenceBundle { study_id: study.id.clone(), node_id, pack, revision, evidence })
}

fn fingerprint(pack: &CoachEvidencePack) -> String {
    let canonical = serde_json::to_vec(pack).unwrap_or_default();
    Sha256::digest(&canonical).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use xq_core::explain::EngineEvidence;

    use super::*;
    use crate::llm::test_support::*;

    #[test]
    fn pack_carries_history_claims_and_quality() {
        let study = study_with(&["h2e2", "h9g7", "h0g2"]);
        let basic = build(&study, 3, &record(&study, 3, None)).unwrap();
        assert_eq!(basic.pack.move_notation, "马二进三");
        assert_eq!(basic.pack.history.moves, vec!["炮二平五", "马8进7"]);
        assert!(basic.pack.history.complete);
        assert_eq!(basic.pack.analysis.level, "basic");
        assert!(basic.pack.analysis.allowed_scope.iter().any(|s| s.contains("不比较候选优劣")));
        // 编号唯一，引用的证据都存在
        let ids: std::collections::HashSet<_> = basic.pack.claims.iter().map(|c| &c.id).collect();
        assert_eq!(ids.len(), basic.pack.claims.len());
        for c in &basic.pack.claims {
            assert!(c.evidence_ids.iter().all(|id| basic.evidence.iter().any(|e| &e.id == id)), "{c:?}");
        }
        assert!(basic.pack.claims.iter().any(|c| c.group == "basis" && c.certainty == Certainty::Certain));

        // 有引擎证据后，证据版本改变；同样内容的版本不变
        let engine = EngineEvidence {
            candidates: vec![line("h0g2", 30, &["h0g2"]), line("b0c2", 20, &["b0c2"])],
            actual: None,
            conditions: "测试".into(),
            complete: true,
            notes: vec![],
        };
        let detailed = build(&study, 3, &record(&study, 3, Some(&engine))).unwrap();
        assert_ne!(detailed.revision, basic.revision);
        assert_eq!(detailed.revision, build(&study, 3, &record(&study, 3, Some(&engine))).unwrap().revision);
        assert_eq!(detailed.pack.candidates.len(), 2);
        assert!(detailed.pack.candidates[0].score.contains("+30"));
        assert!(detailed.pack.claims.iter().any(|c| c.kind == "verdict"));
        let k1 = detailed.evidence.iter().find(|e| e.id == "k1").unwrap();
        assert_eq!(k1.line.as_ref().unwrap().base, LineBase::Parent);

        // 同一条件下重新搜索、只改变评分，证据版本也随之改变
        let rescored = EngineEvidence { candidates: vec![line("h0g2", 35, &["h0g2"]), line("b0c2", 20, &["b0c2"])], ..engine.clone() };
        assert_ne!(build(&study, 3, &record(&study, 3, Some(&rescored))).unwrap().revision, detailed.revision);

        // 不完整的分析：候选只作参考
        let partial = EngineEvidence { complete: false, notes: vec!["最后一轮没有完成".into()], ..engine };
        let p = build(&study, 3, &record(&study, 3, Some(&partial))).unwrap();
        assert_eq!(p.pack.analysis.level, "partial");
        assert!(!p.pack.analysis.complete);
        assert!(p.pack.claims.iter().filter(|c| c.group == "candidate").all(|c| c.certainty == Certainty::Tentative));
    }
}
