// 与后端 serde 结构一一对应的类型。字段名保持后端的 snake_case。

export type Color = "red" | "black";
export type Orientation = "red_bottom" | "black_bottom";
/** ICCS 走法，如 "h2e2" */
export type Move = string;
/** ICCS 坐标，如 "e3" */
export type Square = string;
export type GameStatus = "ongoing" | "checkmate" | "stalemate";
export type Budget = "quick" | "deep";
/** queued 已排队；basic 只有规则事实；partial 有部分引擎证据或某环节失败（可重试）；detailed 完整；failed 无法生成 */
export type ExplainStatus = "queued" | "basic" | "partial" | "detailed" | "failed";
export type StudySource = "standard" | "setup" | "fen" | "record" | "capture";
export type MoveSource = "user" | "record" | "capture" | "variation";

export interface Score {
  kind: "cp" | "mate";
  value: number;
}
export type Bound = "exact" | "lower" | "upper";

export interface RuleSet {
  repetition_rule: string;
  sixty_move_rule: boolean;
}

export interface StudyNode {
  id: number;
  parent: number | null;
  children: number[];
  mv: Move | null;
  notation: string | null;
  fen: string;
  side_to_move: Color;
  ply: number;
  source: MoveSource;
  gap: boolean;
  gap_reason: string | null;
  note: string;
  starred: boolean;
  /** 先想后讲的练习记录（最近一次） */
  practice: Practice | null;
  created_at: number;
}

export interface Practice {
  hints_used: number;
  /** 没有走子就看了讲解 */
  revealed_directly: boolean;
  first_choice: Move | null;
  /** 第一步与棋谱中的这一着相同 */
  matched: boolean;
  sessions: number;
  /** 这次练习看过的提示原文（早期记录没有） */
  hints: UsedHints | null;
  updated_at: number;
}

export interface Study {
  format_version: number;
  id: string;
  name: string;
  created_at: number;
  updated_at: number;
  source: StudySource;
  root_fen: string;
  history_complete: boolean;
  orientation: Orientation;
  user_side: Color;
  rules: RuleSet;
  nodes: Record<string, StudyNode>;
  next_id: number;
  revision: number;
  live_node: number | null;
}

export interface ExplainStats {
  moves: number;
  pending: number;
  basic: number;
  detailed: number;
  partial: number;
  failed: number;
}

export interface StudyView {
  study: Study;
  explain_status: Record<string, ExplainStatus>;
  stats: ExplainStats;
  /** 生成视图时的讲解序号 */
  explain_seq: number;
}

export interface StudySummary {
  id: string;
  name: string;
  source: StudySource;
  created_at: number;
  updated_at: number;
  moves: number;
  root_fen: string;
}

export type EvidenceKind = "fact" | "variation" | "positional";
export type Certainty = "certain" | "supported" | "tentative";

export interface LineRef {
  base: "parent" | "node";
  moves: Move[];
  notation: string[];
}

export interface Evidence {
  id: string;
  kind: EvidenceKind;
  topic: string;
  text: string;
  squares: Square[];
  arrows: Move[];
  line: LineRef | null;
  certainty: Certainty;
  /**
   * squares 与 arrows 所在的局面：parent 为落子前，node 为当前节点的局面。
   * 早期保存的证据没有这一项（null）：所在局面未知，不画在任何棋盘上。
   */
  on: "parent" | "node" | null;
}

export interface Claim {
  text: string;
  evidence: string[];
}

export interface CandidateView {
  mv: Move;
  notation: string;
  score: Score;
  bound: Bound;
  depth: number;
  pv: Move[];
  pv_notation: string[];
  rank: number | null;
  is_actual: boolean;
  source: "multi_pv" | "search_moves";
  summary: string;
}

export type ExplainLevel = "basic" | "partial" | "detailed";

export type VerdictTone = "best" | "close" | "inaccuracy" | "mistake" | "blunder" | "unclear";

export interface Verdict {
  tone: VerdictTone;
  label: string;
  delta: number | null;
  note: string;
}

export interface MoveExplanation {
  generator_version: number;
  level: ExplainLevel;
  mover: Color;
  mv: Move;
  notation: string;
  summary: string;
  need: Claim[];
  basis: Claim[];
  comparison: Claim[];
  replies: Claim[];
  lesson: string;
  verdict: Verdict | null;
  candidates: CandidateView[];
  evidence: Evidence[];
  warnings: string[];
  conditions: string | null;
}

export interface PositionExplanation {
  generator_version: number;
  side: Color;
  status: GameStatus;
  summary: string;
  claims: Claim[];
  evidence: Evidence[];
  candidates: CandidateView[];
  warnings: string[];
  conditions: string | null;
}

export interface ExplanationRecord {
  study_id: string;
  node_id: number;
  explanation: MoveExplanation | null;
  budget: Budget | null;
  conditions_key: string | null;
  pending: boolean;
  error: string | null;
  updated_at: number;
}

export type CloudOutcome =
  | { kind: "moves"; score: number; depth: number; pv: Move[] }
  | { kind: "unknown" }
  | { kind: "invalid_board" }
  | { kind: "checkmate" }
  | { kind: "stalemate" };

export interface CloudRecord {
  fen: string;
  outcome: CloudOutcome;
  pv_notation: string[];
  source: string;
  queried_at: number;
}

export interface NodeDetail {
  study_id: string;
  revision: number;
  node: StudyNode;
  legal_moves: Move[];
  status: GameStatus;
  in_check: boolean;
  history_complete: boolean;
  repetition: number;
  position: PositionExplanation;
  explanation: ExplanationRecord | null;
  /** 走子节点的 AI 讲解状态。 */
  coach: CoachView | null;
  cloud: CloudRecord | null;
  /** 先想后讲的分级提示（只依据这个局面，局面结束时为空） */
  hints: ThinkHints | null;
  /** 这一着在复习中时的状态 */
  review: ReviewStatus | null;
}

export type HintTopic = "in_check" | "own_threat" | "opportunity" | "development" | "improve";

export interface ThinkHints {
  side: Color;
  topic: HintTopic;
  /** 一级：观察局面需求 */
  observe: string;
  /** 二级：相关的棋子与关系 */
  focus: { text: string; squares: Square[]; arrows: Move[] };
  /** 三级：可以考虑的着法（不分先后） */
  moves: { mv: Move; notation: string }[];
  moves_note: string;
}

/** 练习时实际看过的提示：只有用到的级数 */
export interface UsedHints {
  observe: string;
  focus: ThinkHints["focus"] | null;
  moves: ThinkHints["moves"];
}

// ---------- 复习卡片 ----------

/** due 待复习；again 需要再练；understood 已理解 */
export type ReviewStatus = "due" | "again" | "understood";

export interface ReviewMove {
  mv: Move;
  notation: string;
}

/** 加入复习时的思考记录（这一着最近一次先想后讲的练习） */
export interface ReviewThinking {
  first_choice: ReviewMove | null;
  hints_used: number;
  revealed_directly: boolean;
  matched: boolean;
  hints: UsedHints | null;
  practiced_at: number;
}

/** 加入复习时的程序讲解：要点、依据与分析版本 */
export interface ReviewBasis {
  level: ExplainLevel;
  summary: string;
  verdict: Verdict | null;
  need: string[];
  basis: string[];
  replies: string[];
  lesson: string;
  warnings: string[];
  conditions: string | null;
  budget: Budget | null;
  generator_version: number;
  explained_at: number;
}

export interface ReviewAttempt {
  at: number;
  choice: ReviewMove | null;
  matched: boolean;
}

export interface ReviewCard {
  id: string;
  study_id: string;
  node_id: number;
  study_name: string;
  created_at: number;
  updated_at: number;
  status: ReviewStatus;
  /** 落子前局面（含行棋方） */
  fen: string;
  /** 这一着的走棋方 */
  side: Color;
  orientation: Orientation;
  turn: number;
  /** 本段棋谱起点到这个局面的着法 */
  history: ReviewMove[];
  history_complete: boolean;
  /** 棋谱中的这一着 */
  answer: ReviewMove;
  thinking: ReviewThinking | null;
  basis: ReviewBasis | null;
  /** 用户自己的理解或疑问 */
  note: string;
  attempts: ReviewAttempt[];
  reviewed_at: number | null;
}

export interface ReviewItem {
  card: ReviewCard;
  /** 原会话中的这一着仍在，可以打开原局面 */
  source_available: boolean;
  /** 原会话当前的名称 */
  study_name: string;
}

export interface ReviewDetail extends ReviewItem {
  legal_moves: Move[];
}

export interface IllegalMove {
  code: string;
  message: string;
  squares: Square[];
}

export interface MoveResponse {
  node_id: number | null;
  created: boolean;
  illegal: IllegalMove | null;
  study: StudyView | null;
}

export interface RecordStop {
  ply: number;
  token: string;
  reason: string;
}

export interface ImportResponse {
  study: StudyView;
  notes: string[];
  stopped: RecordStop | null;
}

export interface Issue {
  code: string;
  severity: "error" | "warning";
  message: string;
  squares: Square[];
}

export interface ValidationReport {
  ok: boolean;
  issues: Issue[];
  status: GameStatus | null;
  in_check: boolean;
}

export interface ValidateResponse {
  format_error: string | null;
  report: ValidationReport | null;
  side: Color | null;
  needs_side: boolean;
}

export interface EngineIdentity {
  name: string;
  nnue: string;
  score_type: string;
  generation: number;
}

export type EngineState =
  | { state: "starting" }
  | {
      state: "ready";
      identity: EngineIdentity;
      applied: { name: string; value: string }[];
      notes: string[];
    }
  | { state: "failed"; reason: string }
  | { state: "stopped" };

export interface RecognitionStatus {
  available: boolean;
  model: string;
  detail: string;
}

export interface WindowInfo {
  id: number;
  title: string;
  app_name: string;
  width: number;
  height: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Snapshot {
  window: WindowInfo;
  width: number;
  height: number;
  image: string | null;
  suggested: Rect | null;
  note: string | null;
}

export interface ObservedBoard {
  board: string;
  orientation: Orientation | null;
  uncertain: Square[];
  issues: string[];
  changed: Square[];
  suggested_side: Color | null;
}

export type CaptureState =
  | { state: "idle" }
  | { state: "locating"; window: WindowInfo; message: string | null }
  | { state: "awaiting_start"; window: WindowInfo; observed: ObservedBoard }
  | { state: "tracking"; window: WindowInfo; study_id: string; node_id: number; message: string | null }
  | {
      state: "awaiting_confirmation";
      window: WindowInfo;
      study_id: string;
      node_id: number;
      observed: ObservedBoard;
      kind: "unexplained" | "new_game";
      reason: string;
    }
  | { state: "stopped"; window: WindowInfo | null; stage: string; message: string };

export interface AppConfig {
  version: number;
  log_level: string;
  engine: { binary: string | null; nnue: string | null; threads: number; hash_mb: number };
  analysis: {
    quick_time_ms: number;
    quick_depth: number;
    deep_time_ms: number;
    deep_depth: number;
    multipv: number;
    repetition_rule: string;
    sixty_move_rule: boolean;
    auto_explain: boolean;
  };
  capture: {
    interval_ms: number;
    stable_frames: number;
    unexplained_frames: number;
    confidence_threshold: number;
    model: string | null;
  };
  cloud: { enabled: boolean; timeout_ms: number };
  coach: CoachConfig;
}

export type CoachProvider = "openai" | "deepseek" | "ollama";
export type Audience = "beginner" | "intermediate" | "advanced";
export type Detail = "brief" | "normal" | "deep";
export type CoachScope = "off" | "viewed";

/** AI 讲解设置。密钥不在配置中：credential_ref 只是引用（store:coach 或 env:变量名）。 */
export interface CoachConfig {
  enabled: boolean;
  provider: CoachProvider;
  base_url: string;
  model: string;
  credential_ref: string | null;
  timeout_ms: number;
  max_output_tokens: number;
  max_parallel_requests: number;
  daily_request_budget: number;
  auto_explain_scope: CoachScope;
  audience: Audience;
  detail: Detail;
}

export interface CoachAvailability {
  enabled: boolean;
  ready: boolean;
  key_configured: boolean;
  reason: string | null;
  model_label: string;
  used_today: number;
  daily_budget: number;
  /** 生成一着可能调用模型的次数（与后台的重试策略一致） */
  calls_per_move: CoachCallPolicy;
}

/** 生成一着 AI 讲解可能调用模型的次数；每次调用都计入每日次数 */
export interface CoachCallPolicy {
  /** 输出一次通过检查 */
  usual: number;
  /** 输出需要修正时 */
  with_repair: number;
  /** 请求还遇到网络错误、限流或服务端错误而重试时，最多的次数 */
  max: number;
}

export interface AllowedClaim {
  id: string;
  kind: string;
  group: string;
  statement: string;
  evidence_ids: string[];
  certainty: Certainty;
}

export interface CoachText {
  text: string;
  claim_ids: string[];
}

export type SectionKind = "purpose" | "reason" | "comparison" | "reply" | "caution";

export interface CoachSection extends CoachText {
  kind: SectionKind;
}

export interface Followup {
  action: "show_reply" | "show_best" | "compare_alternative";
  label: string;
  evidence_id: string | null;
}

export interface CoachResult {
  output: { summary: CoachText; sections: CoachSection[]; lesson: CoachText; followups: Followup[] };
  claims: AllowedClaim[];
  evidence: Evidence[];
  evidence_revision: string;
  cache_key: string;
  model_label: string;
  audience: Audience;
  detail: Detail;
  created_at: number;
}

export interface CoachRecord {
  study_id: string;
  node_id: number;
  state: "idle" | "queued" | "running" | "ready" | "failed";
  waiting_evidence: boolean;
  generation: number;
  result: CoachResult | null;
  error: string | null;
  updated_at: number;
}

/** disabled 未启用或未配置；none 尚未生成；stale 已有结果但依据或设置已改变。 */
export type CoachStatus = "disabled" | "none" | "queued" | "running" | "ready" | "failed" | "stale";

export interface CoachView {
  /** 状态版本：记录、证据或设置变化时递增，按它判断新旧，不依赖响应到达的先后。 */
  seq: number;
  study_id: string;
  node_id: number;
  status: CoachStatus;
  reason: string | null;
  record: CoachRecord | null;
  /** 记录中保留的结果是否对应当前的证据与设置；否则只能作为“上次的讲解”参考 */
  result_current: boolean;
}

export interface AppStatus {
  version: string;
  engine: EngineState;
  recognition: RecognitionStatus;
  capture: CaptureState;
  config: AppConfig;
  coach: CoachAvailability;
  notices: string[];
  log_dir: string;
  data_dir: string;
}

export type CreateKind =
  | { type: "standard" }
  | { type: "fen"; fen: string; side: Color | null }
  | { type: "setup"; board: string; side: Color }
  | { type: "record"; text: string };

export interface CreateStudyRequest {
  name: string | null;
  kind: CreateKind;
  orientation: Orientation | null;
  user_side: Color | null;
}

// 事件负载
export interface AnalysisProgressEvent {
  request_id: string;
  study_id: string;
  node_id: number;
  depth: number;
  nodes: number;
  time_ms: number;
  candidates: CandidateView[];
}

export interface AnalysisDoneEvent {
  request_id: string;
  study_id: string;
  node_id: number;
  budget: Budget;
  position: PositionExplanation;
  error: string | null;
  cancelled: boolean;
}

export interface ExplanationEvent {
  seq: number;
  study_id: string;
  node_id: number;
  status: ExplainStatus;
  record: ExplanationRecord;
  stats: ExplainStats;
}

export interface StudyUpdatedEvent {
  study_id: string;
  revision: number;
  reason: string;
  new_nodes: number[];
  live_node: number | null;
}

export interface CloudEvent {
  study_id: string;
  node_id: number;
  record: CloudRecord | null;
  error: string | null;
}

// ---------- 试走对比 ----------

export type CompareOutcome = "mine_better" | "reference_better" | "close" | "unclear" | "not_analyzed";
export type ReferenceKind = "actual" | "branch" | "candidate" | "other";

/** 对比中的一方：规则事实、对方最值得关注的回应、从比较起点开始的变化与程序给出的评分说明。 */
export interface CompareSide {
  mv: Move;
  notation: string;
  changes: Claim[];
  reply: Claim | null;
  line: LineRef | null;
  score_text: string | null;
}

export interface MoveComparison {
  generator_version: number;
  mover: Color;
  mine: CompareSide;
  reference: CompareSide;
  outcome: CompareOutcome;
  /** 已确认的差别（程序结论） */
  differences: Claim[];
  evidence: Evidence[];
  warnings: string[];
  conditions: string | null;
}

export interface CompareOption {
  mv: Move;
  notation: string;
  kind: ReferenceKind;
  rank: number | null;
}

export interface CompareView {
  /** 这次对比请求的序号：推送的完整结果沿用请求时的序号，按它判断新旧 */
  seq: number;
  /** 搜索条件的键（引擎不可用时为空） */
  conditions_key: string | null;
  study_id: string;
  /** 比较起点 */
  node_id: number;
  mine: Move;
  reference: Move;
  reference_kind: ReferenceKind;
  reference_rank: number | null;
  /** 同一次搜索尚未完成，完成后推送 comparison_updated */
  pending: boolean;
  error: string | null;
  comparison: MoveComparison;
  options: CompareOption[];
}
