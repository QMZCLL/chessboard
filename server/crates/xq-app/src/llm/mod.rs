//! 基于棋局证据的 AI 讲解（教学层）。
//!
//! 规则层给出棋盘事实，引擎给出候选与变化，模板讲解把它们整理成带编号的结论；
//! 这里把这些内容打包成证据包交给语言模型，由模型组织讲解顺序、衔接与术语解释。
//!
//! - 模型只能引用证据包中“允许表达的结论”，返回的讲解经过结构、引用与硬结论检查才会保存；
//! - 最佳着、失误等级、分差、将杀步数等关键结果仍由模板讲解显示，模型不能改写；
//! - 网络请求在独立的工作线程中进行；结果回到讲解线程，按代次、证据版本与模型设置检查后写入；
//! - 模型服务不可用或结果未通过检查时，模板讲解照常可用。

pub mod credentials;
pub mod evidence;
pub mod prompt;
pub mod provider;
pub(crate) mod service;
#[cfg(test)]
mod test_support;
pub mod validate;

use serde::Deserialize;
use serde::Serialize;
use xq_core::explain::Evidence;
use xq_core::study::NodeId;

use crate::config::Audience;
use crate::config::Detail;
use evidence::AllowedClaim;

/// 讲解中的一段文字及其引用的结论编号。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoachText {
    pub text: String,
    pub claim_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    /// 当前需要解决的问题与本步的作用。
    Purpose,
    /// 本步具体改变了什么。
    Reason,
    /// 与其他候选的比较。
    Comparison,
    /// 对方值得注意的回应。
    Reply,
    /// 需要注意的风险。
    Caution,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoachSection {
    pub kind: SectionKind,
    pub text: String,
    pub claim_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowupAction {
    /// 演示对方的主要应对。
    ShowReply,
    /// 演示首选候选的变化。
    ShowBest,
    /// 演示另一个候选。
    CompareAlternative,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Followup {
    pub action: FollowupAction,
    pub label: String,
    /// 由后台根据证据补充，界面据此回放；模型不提供。
    #[serde(default)]
    pub evidence_id: Option<String>,
}

/// 通过检查的 AI 讲解。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoachExplanation {
    pub summary: CoachText,
    pub sections: Vec<CoachSection>,
    pub lesson: CoachText,
    pub followups: Vec<Followup>,
}

/// 一次成功生成的结果，连同它引用的结论与证据快照一起保存：
/// 证据之后更新时，界面仍能按生成时的依据高亮和回放，并提示讲解已过期。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoachResult {
    pub output: CoachExplanation,
    pub claims: Vec<AllowedClaim>,
    pub evidence: Vec<Evidence>,
    pub evidence_revision: String,
    pub cache_key: String,
    pub model_label: String,
    pub audience: Audience,
    pub detail: Detail,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoachState {
    /// 没有进行中的请求。
    Idle,
    /// 已排队（可能在等待引擎分析完成）。
    Queued,
    /// 已发出请求，等待模型返回。
    Running,
    /// 最近一次请求成功。
    Ready,
    /// 最近一次请求失败；已有的结果保留。
    Failed,
}

/// 某一着的 AI 讲解记录。与逐着讲解记录分开保存：AI 讲解成功不会改变分析状态。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoachRecord {
    pub study_id: String,
    pub node_id: NodeId,
    pub state: CoachState,
    /// 排队时是否在等待引擎分析完成。
    pub waiting_evidence: bool,
    pub generation: u64,
    pub result: Option<CoachResult>,
    pub error: Option<String>,
    pub updated_at: i64,
}

impl CoachRecord {
    pub fn new(study_id: &str, node_id: NodeId, now: i64) -> Self {
        Self {
            study_id: study_id.to_string(),
            node_id,
            state: CoachState::Idle,
            waiting_evidence: false,
            generation: 0,
            result: None,
            error: None,
            updated_at: now,
        }
    }
}

/// 界面看到的 AI 讲解状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoachStatus {
    /// 没有启用或没有配置好模型服务。
    Disabled,
    /// 尚未生成。
    None,
    Queued,
    Running,
    /// 结果对应当前的证据、模型与讲解设置。
    Ready,
    Failed,
    /// 已有结果，但证据、模型或讲解设置已经改变。
    Stale,
}

#[derive(Clone, Debug, Serialize)]
pub struct CoachView {
    pub study_id: String,
    pub node_id: NodeId,
    pub status: CoachStatus,
    /// 状态说明（未启用的原因、失败原因、过期原因、等待引擎等）。
    pub reason: Option<String>,
    pub record: Option<CoachRecord>,
}
