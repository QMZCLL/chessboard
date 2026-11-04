//! 安装包的冒烟测试：`xqlink --smoke <报告文件> [--data <目录>]`。
//!
//! 不打开窗口：按应用自己的目录（或 `--data` 指定的目录）启动服务，等引擎就绪（最多 60 秒），检查数据库能打开、
//! 迁移是否完成、随安装包发布的引擎与许可文件是否都在，把结果写成 JSON 报告后退出。全部通过时退出码为 0，否则为 1。
//! Windows 发布版没有控制台，结果只写在报告文件中。
//!
//! 应用目录与 Tauri 的规则相同（tauri/src/path/desktop.rs）：数据目录为 `dirs::data_dir()/标识符`，
//! 配置目录为 `dirs::config_dir()/标识符`，日志目录在 macOS 上为 `~/Library/Logs/标识符`，其他系统为
//! `dirs::data_local_dir()/标识符/logs`；资源目录用 `tauri::utils::platform::resource_dir`。

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use serde_json::json;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::engine::EngineState;
use xq_app::events::EventSink;

/// 命令行中的冒烟测试参数：报告文件与可选的数据目录。
pub fn requested() -> Option<(PathBuf, Option<PathBuf>)> {
    let args: Vec<String> = std::env::args().collect();
    let at = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(PathBuf::from);
    at("--smoke").map(|report| (report, at("--data")))
}

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: &str, _payload: serde_json::Value) {}
}

fn paths(identifier: &str, package: &tauri::PackageInfo, data: Option<&Path>) -> Result<AppPaths, String> {
    let resource_dir = tauri::utils::platform::resource_dir(package, &tauri::Env::default()).map_err(|e| format!("找不到资源目录：{e}"))?;
    if let Some(root) = data {
        return Ok(AppPaths {
            data_dir: root.join("data"),
            config_dir: root.join("config"),
            log_dir: root.join("logs"),
            resource_dir,
            legacy_config: None,
        });
    }
    let unknown = || "找不到用户目录".to_string();
    #[cfg(target_os = "macos")]
    let log_dir = dirs::home_dir().ok_or_else(unknown)?.join("Library/Logs").join(identifier);
    #[cfg(not(target_os = "macos"))]
    let log_dir = dirs::data_local_dir().ok_or_else(unknown)?.join(identifier).join("logs");
    Ok(AppPaths {
        data_dir: dirs::data_dir().ok_or_else(unknown)?.join(identifier),
        config_dir: dirs::config_dir().ok_or_else(unknown)?.join(identifier),
        log_dir,
        resource_dir,
        legacy_config: dirs::config_dir().map(|d| d.join("xqlink").join("config.json")),
    })
}

/// 运行冒烟测试，返回退出码。
pub fn run(identifier: &str, package: &tauri::PackageInfo, report: &Path, data: Option<&Path>) -> i32 {
    let started = Instant::now();
    let mut problems: Vec<String> = Vec::new();
    let mut out = json!({ "version": package.version.to_string(), "identifier": identifier });
    match paths(identifier, package, data) {
        Err(e) => problems.push(e),
        Ok(paths) => {
            out["data_dir"] = json!(paths.data_dir.display().to_string());
            out["resource_dir"] = json!(paths.resource_dir.display().to_string());
            let engine = paths.bundled_engine();
            let licenses = paths.resource_dir.join("THIRD_PARTY_LICENSES.md");
            out["bundled_engine"] = json!(engine.exists());
            out["third_party_licenses"] = json!(licenses.exists());
            if !engine.exists() {
                problems.push(format!("缺少随安装包发布的引擎 {}", engine.display()));
            }
            if !licenses.exists() {
                problems.push(format!("缺少第三方许可清单 {}", licenses.display()));
            }
            let backend = Arc::new(crate::recognition::XcapBackend::new(&paths.model_dir(), None));
            match AppService::start(paths, Arc::new(Quiet), backend) {
                Err(e) => problems.push(format!("服务启动失败：{}", e.message)),
                Ok(service) => {
                    let deadline = Instant::now() + Duration::from_secs(60);
                    while matches!(service.status().engine, EngineState::Starting) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    let status = service.status();
                    if !matches!(status.engine, EngineState::Ready { .. }) {
                        problems.push(format!("引擎没有就绪：{:?}", status.engine));
                    }
                    out["app_version"] = json!(status.version);
                    if status.version != package.version.to_string() {
                        problems.push(format!("应用内版本 {} 与安装包版本 {} 不一致", status.version, package.version));
                    }
                    out["engine"] = serde_json::to_value(&status.engine).unwrap_or_default();
                    out["recognition"] = json!({ "available": status.recognition.available, "detail": status.recognition.detail });
                    out["notices"] = json!(status.notices);
                    out["schema"] = serde_json::to_value(service.schema_status()).unwrap_or_default();
                    out["studies"] = json!(service.list_studies().map(|s| s.len()).unwrap_or(0));
                    out["reviews"] = json!(service.list_reviews().map(|r| r.len()).unwrap_or(0));
                    service.shutdown();
                }
            }
        }
    }
    out["ok"] = json!(problems.is_empty());
    out["problems"] = json!(problems);
    out["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
    let text = serde_json::to_string_pretty(&out).unwrap_or_default();
    if let Some(dir) = report.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(report, &text) {
        eprintln!("写入报告 {} 失败：{e}", report.display());
        return 1;
    }
    println!("{text}");
    if problems.is_empty() { 0 } else { 1 }
}
