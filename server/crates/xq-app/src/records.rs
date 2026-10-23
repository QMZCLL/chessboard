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

/// 分析记录的格式版本。结果结构或完整性判断改变时递增：旧格式的记录既不复用也不展示，
/// 下次搜索时被覆盖。版本 1 起结果带有完整性信息。
pub const ANALYSIS_FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRecord {
    pub key: String,
    pub conditions: AnalysisConditions,
    pub outcome: AnalysisOutcome,
    /// 早期记录没有该字段，读作 0。
    #[serde(default)]
    pub format: u32,
    pub created_at: i64,
}

impl AnalysisRecord {
    pub fn is_current_format(&self) -> bool { self.format == ANALYSIS_FORMAT }

    /// 能否代替一次新的搜索：当前格式，且是终局或完整一致的结果。
    /// 不完整的结果只用于展示，再次请求时总是重新搜索。
    pub fn reusable(&self) -> bool {
        self.is_current_format()
            && match &self.outcome {
                AnalysisOutcome::Completed { result } => result.complete,
                AnalysisOutcome::Terminal { .. } => true,
            }
    }
}

/// 某个走子节点的讲解状态。排队中的任务不计为已讲解，缺少环节的讲解不计为完成。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplainStatus {
    /// 已建立任务，还没有任何讲解。
    Queued,
    /// 只有规则事实（引擎分析尚未开始或进行中）。
    Basic,
    /// 有部分引擎证据，或某个环节失败；保留已有内容，可以重试。
    Partial,
    /// 候选比较与实际走法分析都已完成。
    Detailed,
    /// 无法生成任何讲解。
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplanationRecord {
    pub study_id: String,
    pub node_id: NodeId,
    pub explanation: Option<MoveExplanation>,
    /// 引擎证据所用的分析预算。
    pub budget: Option<Budget>,
    /// 引擎证据所用分析条件的键；设置改变后据此判断讲解是否需要更新。
    #[serde(default)]
    pub conditions_key: Option<String>,
    /// 是否还有未完成的引擎任务。
    pub pending: bool,
    /// 未完成环节的原因。
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
            conditions_key: None,
            pending: false,
            error: None,
            updated_at: now,
        }
    }

    pub fn status(&self) -> ExplainStatus {
        let failed = self.error.is_some() && !self.pending;
        match &self.explanation {
            None if failed => ExplainStatus::Failed,
            None => ExplainStatus::Queued,
            Some(e) => match e.level {
                ExplainLevel::Detailed => ExplainStatus::Detailed,
                ExplainLevel::Partial => ExplainStatus::Partial,
                ExplainLevel::Basic if failed => ExplainStatus::Partial,
                ExplainLevel::Basic => ExplainStatus::Basic,
            },
        }
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
