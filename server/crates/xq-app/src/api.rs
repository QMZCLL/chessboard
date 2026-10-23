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
use crate::engine::EngineState;
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
