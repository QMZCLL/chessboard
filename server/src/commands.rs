//! Tauri 命令：只做参数转换，业务逻辑都在 `xq_app::AppService`。
//! 可能阻塞的操作放到阻塞线程池执行，避免占用异步运行时或界面线程。

use std::path::PathBuf;

use tauri::State;
use xq_app::ApiError;
use xq_app::ApiResult;
use xq_app::AppService;
use xq_app::api::*;
use xq_app::capture::CaptureState;
use xq_app::compare::CompareRequest;
use xq_app::compare::CompareView;
use xq_app::capture::Snapshot;
use xq_app::capture::WindowInfo;
use xq_app::config::AppConfig;
use xq_app::llm::CoachView;
use xq_app::llm::ask::AnswerView;
use xq_app::llm::ask::AnswersRequest;
use xq_app::llm::ask::Question;
use xq_app::records::ExplanationRecord;
use xq_app::review::ReviewAttemptRequest;
use xq_app::review::ReviewDetail;
use xq_app::review::ReviewItem;
use xq_app::review::ReviewRef;
use xq_app::review::ReviewUpdateRequest;

use crate::StartupError;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| ApiError::from(format!("内部错误：{e}")))?
}

fn service(state: &State<'_, AppService>) -> AppService { state.inner().clone() }

#[tauri::command]
pub fn startup_error(state: State<'_, StartupError>) -> Option<String> { state.0.lock().ok().and_then(|e| e.clone()) }

#[tauri::command]
pub async fn app_status(state: State<'_, AppService>) -> ApiResult<AppStatus> { Ok(service(&state).status()) }

#[tauri::command]
pub async fn update_config(state: State<'_, AppService>, config: AppConfig) -> ApiResult<AppStatus> {
    let s = service(&state);
    let level = config.log_level.clone();
    let status = blocking(move || s.update_config(config)).await?;
    crate::logger::apply_level(&level);
    Ok(status)
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, AppService>) -> ApiResult<()> {
    service(&state).restart_engine();
    Ok(())
}

#[tauri::command]
pub async fn list_studies(state: State<'_, AppService>) -> ApiResult<Vec<StudySummary>> {
    let s = service(&state);
    blocking(move || s.list_studies()).await
}

#[tauri::command]
pub async fn create_study(state: State<'_, AppService>, req: CreateStudyRequest) -> ApiResult<ImportResponse> {
    let s = service(&state);
    blocking(move || s.create_study(req)).await
}

#[tauri::command]
pub async fn validate_position(state: State<'_, AppService>, req: ValidateRequest) -> ApiResult<ValidateResponse> {
    Ok(service(&state).validate_position(req))
}

#[tauri::command]
pub async fn load_study(state: State<'_, AppService>, id: String) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.load_study(&id)).await
}

#[tauri::command]
pub async fn study_view(state: State<'_, AppService>, id: String) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.study_view(&id)).await
}

#[tauri::command]
pub async fn update_study(state: State<'_, AppService>, req: StudyPrefsRequest) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.update_study(req)).await
}

#[tauri::command]
pub async fn delete_study(state: State<'_, AppService>, id: String) -> ApiResult<()> {
    let s = service(&state);
    blocking(move || s.delete_study(&id)).await
}

#[tauri::command]
pub async fn node_detail(state: State<'_, AppService>, req: NodeRef) -> ApiResult<NodeDetail> {
    let s = service(&state);
    blocking(move || s.node_detail(req)).await
}

#[tauri::command]
pub async fn apply_move(state: State<'_, AppService>, req: MoveRequest) -> ApiResult<MoveResponse> {
    let s = service(&state);
    blocking(move || s.apply_move(req)).await
}

#[tauri::command]
pub async fn add_line(state: State<'_, AppService>, req: LineRequest) -> ApiResult<MoveResponse> {
    let s = service(&state);
    blocking(move || s.add_line(req)).await
}

#[tauri::command]
pub async fn delete_node(state: State<'_, AppService>, req: NodeRef) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.delete_node(req)).await
}

#[tauri::command]
pub async fn promote_node(state: State<'_, AppService>, req: NodeRef) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.promote_node(req)).await
}

#[tauri::command]
pub async fn set_note(state: State<'_, AppService>, req: NoteRequest) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.set_note(req)).await
}

#[tauri::command]
pub async fn record_practice(state: State<'_, AppService>, req: PracticeRequest) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.record_practice(req)).await
}

#[tauri::command]
pub async fn focus_node(state: State<'_, AppService>, req: NodeRef) -> ApiResult<()> {
    service(&state).focus_node(req);
    Ok(())
}

#[tauri::command]
pub async fn coach_explain(state: State<'_, AppService>, req: CoachExplainRequest) -> ApiResult<CoachView> {
    let s = service(&state);
    blocking(move || s.coach_explain(req)).await
}

#[tauri::command]
pub async fn coach_cancel(state: State<'_, AppService>, req: NodeRef) -> ApiResult<CoachView> {
    let s = service(&state);
    blocking(move || s.coach_cancel(req)).await
}

#[tauri::command]
pub async fn coach_leave(state: State<'_, AppService>, req: NodeRef) -> ApiResult<CoachView> {
    let s = service(&state);
    blocking(move || s.coach_leave(req)).await
}

#[tauri::command]
pub async fn coach_view(state: State<'_, AppService>, req: NodeRef) -> ApiResult<CoachView> {
    let s = service(&state);
    blocking(move || s.coach_view(req)).await
}

#[tauri::command]
pub async fn set_coach_credential(state: State<'_, AppService>, req: CoachCredentialRequest) -> ApiResult<AppStatus> {
    let s = service(&state);
    blocking(move || s.set_coach_credential(req)).await
}

#[tauri::command]
pub async fn explain_move(state: State<'_, AppService>, req: ExplainRequest) -> ApiResult<Option<ExplanationRecord>> {
    let s = service(&state);
    blocking(move || s.explain_move(req)).await
}

#[tauri::command]
pub async fn analyze_position(state: State<'_, AppService>, req: AnalyzeRequest) -> ApiResult<()> {
    let s = service(&state);
    blocking(move || s.analyze_position(req)).await
}

#[tauri::command]
pub async fn compare_moves(state: State<'_, AppService>, req: CompareRequest) -> ApiResult<CompareView> {
    let s = service(&state);
    blocking(move || s.compare_moves(req)).await
}

#[tauri::command]
pub async fn ask_coach(state: State<'_, AppService>, req: Question) -> ApiResult<AnswerView> {
    let s = service(&state);
    blocking(move || s.ask_coach(req)).await
}

#[tauri::command]
pub async fn coach_answers(state: State<'_, AppService>, req: AnswersRequest) -> ApiResult<Vec<AnswerView>> {
    let s = service(&state);
    blocking(move || s.coach_answers(req)).await
}

#[tauri::command]
pub async fn add_review(state: State<'_, AppService>, req: NodeRef) -> ApiResult<ReviewItem> {
    let s = service(&state);
    blocking(move || s.add_review(req)).await
}

#[tauri::command]
pub async fn list_reviews(state: State<'_, AppService>) -> ApiResult<Vec<ReviewItem>> {
    let s = service(&state);
    blocking(move || s.list_reviews()).await
}

#[tauri::command]
pub async fn review_detail(state: State<'_, AppService>, req: ReviewRef) -> ApiResult<ReviewDetail> {
    let s = service(&state);
    blocking(move || s.review_detail(req)).await
}

#[tauri::command]
pub async fn review_attempt(state: State<'_, AppService>, req: ReviewAttemptRequest) -> ApiResult<ReviewItem> {
    let s = service(&state);
    blocking(move || s.review_attempt(req)).await
}

#[tauri::command]
pub async fn update_review(state: State<'_, AppService>, req: ReviewUpdateRequest) -> ApiResult<ReviewItem> {
    let s = service(&state);
    blocking(move || s.update_review(req)).await
}

#[tauri::command]
pub async fn delete_review(state: State<'_, AppService>, req: ReviewRef) -> ApiResult<()> {
    let s = service(&state);
    blocking(move || s.delete_review(req)).await
}

#[tauri::command]
pub async fn cancel_analysis(state: State<'_, AppService>, request_id: String) -> ApiResult<()> {
    service(&state).cancel_analysis(&request_id);
    Ok(())
}

#[tauri::command]
pub async fn cloud_lookup(state: State<'_, AppService>, req: NodeRef) -> ApiResult<()> {
    let s = service(&state);
    blocking(move || s.cloud_lookup(req)).await
}

#[tauri::command]
pub async fn export_study_to_file(state: State<'_, AppService>, id: String, path: PathBuf) -> ApiResult<()> {
    let s = service(&state);
    blocking(move || s.export_study_to_file(&id, &path)).await
}

#[tauri::command]
pub async fn import_file(state: State<'_, AppService>, path: PathBuf) -> ApiResult<ImportResponse> {
    let s = service(&state);
    blocking(move || s.import_file(&path)).await
}

#[tauri::command]
pub async fn import_text(state: State<'_, AppService>, text: String, name: Option<String>) -> ApiResult<ImportResponse> {
    let s = service(&state);
    blocking(move || s.import_text(&text, name)).await
}

#[tauri::command]
pub async fn diagnostics(state: State<'_, AppService>) -> ApiResult<String> { Ok(service(&state).diagnostics()) }

#[tauri::command]
pub async fn open_log_dir(state: State<'_, AppService>) -> ApiResult<()> {
    let dir = service(&state).log_dir();
    std::fs::create_dir_all(&dir).map_err(|e| ApiError::from(format!("无法创建日志目录：{e}")))?;
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(|e| ApiError::from(format!("无法打开日志目录：{e}")))
}

#[tauri::command]
pub async fn list_windows(state: State<'_, AppService>) -> ApiResult<Vec<WindowInfo>> {
    let s = service(&state);
    blocking(move || s.list_windows()).await
}

#[tauri::command]
pub async fn capture_snapshot(state: State<'_, AppService>, window_id: u32) -> ApiResult<Snapshot> {
    let s = service(&state);
    blocking(move || s.capture_snapshot(window_id)).await
}

#[tauri::command]
pub async fn start_capture(state: State<'_, AppService>, req: StartCaptureRequest) -> ApiResult<CaptureState> {
    let s = service(&state);
    blocking(move || s.start_capture(req)).await
}

#[tauri::command]
pub async fn stop_capture(state: State<'_, AppService>) -> ApiResult<CaptureState> {
    let s = service(&state);
    blocking(move || Ok(s.stop_capture())).await
}

#[tauri::command]
pub async fn confirm_capture_start(state: State<'_, AppService>, req: ConfirmStartRequest) -> ApiResult<StudyView> {
    let s = service(&state);
    blocking(move || s.confirm_capture_start(req)).await
}

#[tauri::command]
pub async fn resolve_capture(state: State<'_, AppService>, req: ResolveCaptureRequest) -> ApiResult<CaptureState> {
    let s = service(&state);
    blocking(move || s.resolve_capture(req)).await
}

#[tauri::command]
pub async fn relocate_capture(state: State<'_, AppService>) -> ApiResult<CaptureState> {
    let s = service(&state);
    blocking(move || s.relocate_capture()).await
}
