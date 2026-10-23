//! 界面与服务之间的请求和响应结构。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;
use xq_core::Color;
use xq_core::Move;
use xq_core::capture::Rect;
use xq_core::explain::PositionExplanation;
use xq_core::record::RecordStop;
use xq_core::rules::GameStatus;
use xq_core::rules::IllegalMove;
use xq_core::study::Node;
use xq_core::study::NodeId;
use xq_core::study::Orientation;
use xq_core::study::Study;
use xq_core::study::StudySource;
use xq_core::validate::ValidationReport;

use crate::capture::CaptureState;
use crate::capture::RecognitionStatus;
use crate::config::AppConfig;
use crate::config::Audience;
use crate::config::Detail;
use crate::engine::EngineState;
use crate::llm::CoachView;
use crate::records::Budget;
use crate::records::CloudRecord;
use crate::records::ExplainStatus;
use crate::records::ExplanationRecord;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreateKind {
    Standard,
    /// FEN 导入；FEN 不含行棋方时必须给出 `side`。
    Fen { fen: String, side: Option<Color> },
    /// 手动摆棋：棋子布局加明确选择的行棋方。
    Setup { board: String, side: Color },
    Record { text: String },
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateStudyRequest {
    pub name: Option<String>,
    pub kind: CreateKind,
    pub orientation: Option<Orientation>,
    pub user_side: Option<Color>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ValidateRequest {
    /// 棋子布局或完整 FEN。
    pub fen: String,
    pub side: Option<Color>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ValidateResponse {
    pub format_error: Option<String>,
    /// 给出行棋方后的完整校验。
    pub report: Option<ValidationReport>,
    pub side: Option<Color>,
    pub needs_side: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NodeRef {
    pub study_id: String,
    pub node_id: NodeId,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MoveRequest {
    pub study_id: String,
    pub parent: NodeId,
    pub mv: Move,
    /// 试走（建立分支并立即讲解）还是记录主线。
    #[serde(default)]
    pub variation: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LineRequest {
    pub study_id: String,
    pub parent: NodeId,
    pub moves: Vec<Move>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AnalyzeRequest {
    pub study_id: String,
    pub node_id: NodeId,
    pub budget: Budget,
    pub request_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExplainRequest {
    pub study_id: String,
    pub node_id: NodeId,
    pub budget: Budget,
    /// 用户明确要求重试：部分完成的讲解重新分析，不复用不完整的结果。
    #[serde(default)]
    pub retry: bool,
}

/// 生成某一着的 AI 讲解。证据、局面与搜索结果都由后台读取，不由界面传入。
#[derive(Clone, Debug, Deserialize)]
pub struct CoachExplainRequest {
    pub study_id: String,
    pub node_id: NodeId,
    /// 为空时使用设置中的学习水平与详细程度。
    #[serde(default)]
    pub audience: Option<Audience>,
    #[serde(default)]
    pub detail: Option<Detail>,
    /// 忽略已有结果与缓存，重新生成。
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CoachCredentialRequest {
    /// 为空时清除已保存的密钥。
    pub key: Option<String>,
}

/// AI 讲解是否可用。只说明密钥是否已配置，不返回密钥本身。
#[derive(Clone, Debug, Serialize)]
pub struct CoachAvailability {
    pub enabled: bool,
    pub ready: bool,
    pub key_configured: bool,
    pub reason: Option<String>,
    pub model_label: String,
    pub used_today: u32,
    pub daily_budget: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NoteRequest {
    pub study_id: String,
    pub node_id: NodeId,
    pub note: String,
    pub starred: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StudyPrefsRequest {
    pub study_id: String,
    pub name: Option<String>,
    pub orientation: Option<Orientation>,
    pub user_side: Option<Color>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ExplainStats {
    /// 走子节点总数。
    pub moves: usize,
    /// 已建立但尚未完成的讲解任务。
    pub pending: usize,
    /// 至少有基本讲解的走子数。
    pub basic: usize,
    pub detailed: usize,
    /// 有部分引擎证据或某个环节失败、可以重试的走子数。
    pub partial: usize,
    /// 无法生成任何讲解的走子数。
    pub failed: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct StudyView {
    pub study: Study,
    pub explain_status: BTreeMap<NodeId, ExplainStatus>,
    pub stats: ExplainStats,
    /// 生成视图时的讲解序号，见 `explanation_updated` 事件的 `seq`。
    pub explain_seq: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StudySummary {
    pub id: String,
    pub name: String,
    pub source: StudySource,
    pub created_at: i64,
    pub updated_at: i64,
    pub moves: usize,
    pub root_fen: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct NodeDetail {
    pub study_id: String,
    pub revision: u64,
    pub node: Node,
    pub legal_moves: Vec<Move>,
    pub status: GameStatus,
    pub in_check: bool,
    pub history_complete: bool,
    pub repetition: usize,
    /// 当前局面说明（含已缓存的引擎候选）。
    pub position: PositionExplanation,
    /// 走子节点的逐着讲解。
    pub explanation: Option<ExplanationRecord>,
    /// 走子节点的 AI 讲解状态。
    pub coach: Option<CoachView>,
    pub cloud: Option<CloudRecord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MoveResponse {
    pub node_id: Option<NodeId>,
    pub created: bool,
    pub illegal: Option<IllegalMove>,
    pub study: Option<StudyView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportResponse {
    pub study: StudyView,
    pub notes: Vec<String>,
    pub stopped: Option<RecordStop>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppStatus {
    pub version: String,
    pub engine: EngineState,
    pub recognition: RecognitionStatus,
    pub capture: CaptureState,
    pub config: AppConfig,
    pub coach: CoachAvailability,
    pub notices: Vec<String>,
    pub log_dir: String,
    pub data_dir: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StartCaptureRequest {
    pub window_id: u32,
    pub rect: Rect,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfirmStartRequest {
    /// 校正后的棋子布局（标准方向）。
    pub board: String,
    pub side: Color,
    pub orientation: Orientation,
    pub name: Option<String>,
    /// 接到已有学习会话的某个节点之后（作为缺口节点），否则新建会话。
    pub attach: Option<NodeRefOwned>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NodeRefOwned {
    pub study_id: String,
    pub node_id: NodeId,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ResolveCaptureRequest {
    /// 以画面局面建立缺口节点继续记录。
    Accept { board: String, side: Color },
    /// 新开一个学习会话记录新的一局。
    NewStudy { board: String, side: Color },
    /// 暂不处理，继续等待。
    Dismiss,
}
