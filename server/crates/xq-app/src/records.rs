//! 持久化的分析与讲解记录。

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use xq_core::Move;
use xq_core::cloud::CloudOutcome;
use xq_core::explain::ExplainLevel;
use xq_core::explain::MoveExplanation;
use xq_core::rules::GameStatus;
use xq_core::study::NodeId;
use xq_core::study::RuleSet;

use crate::engine::SearchResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Budget {
    Quick,
    Deep,
}

impl Budget {
    pub fn label(self) -> &'static str {
        match self {
            Budget::Quick => "快速分析",
            Budget::Deep => "深入分析",
        }
    }
}

/// 一次搜索的完整条件。缓存键由全部条件决定：局面、历史、规则、根走法限制、
/// 引擎与网络文件、评分类型和搜索预算任何一项不同都不能复用结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisConditions {
    pub base_fen: String,
    pub moves: Vec<Move>,
    pub searchmoves: Vec<Move>,
    pub budget: Budget,
    pub multipv: u32,
    pub depth: u32,
    pub movetime_ms: u64,
    pub rules: RuleSet,
    pub engine: String,
    pub nnue: String,
    pub score_type: String,
}

impl AnalysisConditions {
    pub fn key(&self) -> String {
        let canonical = serde_json::to_string(self).unwrap_or_default();
        let digest = Sha256::digest(canonical.as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn describe(&self) -> String {
        let mut text = format!(
            "{} · {}（≤{:.1} 秒，深度≤{}）· {} 个候选 · {}",
            self.engine,
            self.budget.label(),
            self.movetime_ms as f64 / 1000.0,
            self.depth,
            self.multipv,
            self.rules.repetition_rule
        );
        if !self.searchmoves.is_empty() {
            text.push_str(" · 指定着法单独分析");
        }
        text
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnalysisOutcome {
    Completed { result: SearchResult },
    Terminal { status: GameStatus },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRecord {
    pub key: String,
    pub conditions: AnalysisConditions,
    pub outcome: AnalysisOutcome,
    pub created_at: i64,
}

/// 某个走子节点的讲解状态。“已建任务”“已有基本解释”“详细解释完成”分别记录，
/// 排队中的任务不计为已讲解。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplainStatus {
    Queued,
    Basic,
    Detailed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplanationRecord {
    pub study_id: String,
    pub node_id: NodeId,
    pub explanation: Option<MoveExplanation>,
    /// 详细讲解所用的分析预算。
    pub budget: Option<Budget>,
    /// 是否还有未完成的引擎任务。
    pub pending: bool,
    pub error: Option<String>,
    pub updated_at: i64,
}

impl ExplanationRecord {
    pub fn new(study_id: &str, node_id: NodeId, now: i64) -> Self {
        Self {
            study_id: study_id.to_string(),
            node_id,
            explanation: None,
            budget: None,
            pending: false,
            error: None,
            updated_at: now,
        }
    }

    pub fn status(&self) -> ExplainStatus {
        match (&self.explanation, &self.error) {
            (Some(e), _) if e.level == ExplainLevel::Detailed => ExplainStatus::Detailed,
            (_, Some(_)) if !self.pending => ExplainStatus::Failed,
            (Some(_), _) => ExplainStatus::Basic,
            (None, _) => ExplainStatus::Queued,
        }
    }

    pub fn is_detailed_for(&self, budget: Budget) -> bool {
        self.status() == ExplainStatus::Detailed && (budget == Budget::Quick || self.budget == Some(Budget::Deep))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudRecord {
    pub fen: String,
    pub outcome: CloudOutcome,
    pub pv_notation: Vec<String>,
    pub source: String,
    pub queried_at: i64,
}
