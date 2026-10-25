use std::sync::Arc;
use std::sync::Mutex;

use tauri::Emitter as _;
use tauri::Manager as _;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::events::EventSink;

mod commands;
mod logger;
mod recognition;

/// 服务启动失败的原因；界面启动时读取并提示用户。
pub struct StartupError(pub Mutex<Option<String>>);

struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Err(e) = self.0.emit(event, payload) {
            tracing::warn!("推送事件 {event} 失败：{e}");
        }
    }
}

fn resolve_paths(handle: &tauri::AppHandle) -> Result<AppPaths, String> {
    let p = handle.path();
    Ok(AppPaths {
        data_dir: p.app_data_dir().map_err(|e| e.to_string())?,
        config_dir: p.app_config_dir().map_err(|e| e.to_string())?,
        log_dir: p.app_log_dir().map_err(|e| e.to_string())?,
        resource_dir: p.resource_dir().map_err(|e| e.to_string())?,
        legacy_config: p.config_dir().ok().map(|d| d.join("xqlink").join("config.json")),
    })
}

fn start_service(handle: &tauri::AppHandle) -> Result<AppService, String> {
    let paths = resolve_paths(handle)?;
    logger::init(&paths.log_dir);
    let model_override = xq_app::config::peek(&paths.config_dir, paths.legacy_config.as_deref()).capture.model;
    let backend = Arc::new(recognition::XcapBackend::new(&paths.model_dir(), model_override));
    let service = AppService::start(paths, Arc::new(TauriSink(handle.clone())), backend).map_err(|e| e.message)?;
    logger::apply_level(&service.status().config.log_level);
    Ok(service)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let startup = StartupError(Mutex::new(None));
            match start_service(app.handle()) {
                Ok(service) => {
                    app.manage(service);
                }
                Err(e) => {
                    tracing::error!("服务启动失败：{e}");
                    *startup.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
                }
            }
            app.manage(startup);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::startup_error,
            commands::app_status,
            commands::update_config,
            commands::restart_engine,
            commands::list_studies,
            commands::create_study,
            commands::validate_position,
            commands::load_study,
            commands::study_view,
            commands::update_study,
            commands::delete_study,
            commands::node_detail,
            commands::apply_move,
            commands::add_line,
            commands::delete_node,
            commands::promote_node,
            commands::set_note,
            commands::record_practice,
            commands::focus_node,
            commands::explain_move,
            commands::coach_explain,
            commands::coach_cancel,
            commands::coach_leave,
            commands::coach_view,
            commands::set_coach_credential,
            commands::analyze_position,
            commands::compare_moves,
            commands::add_review,
            commands::list_reviews,
            commands::review_detail,
            commands::review_attempt,
            commands::update_review,
            commands::delete_review,
            commands::cancel_analysis,
            commands::cloud_lookup,
            commands::export_study_to_file,
            commands::import_file,
            commands::import_text,
            commands::diagnostics,
            commands::open_log_dir,
            commands::list_windows,
            commands::capture_snapshot,
            commands::start_capture,
            commands::stop_capture,
            commands::confirm_capture_start,
            commands::resolve_capture,
            commands::relocate_capture,
        ])
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            eprintln!("应用初始化失败：{e}");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(service) = handle.try_state::<AppService>()
        {
            service.shutdown();
        }
    });
}
