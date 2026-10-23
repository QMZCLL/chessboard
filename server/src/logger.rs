use std::path::Path;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::RollingFileAppender;
use tracing_appender::rolling::Rotation;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Registry;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static FILTER: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();

fn directives(level: &str) -> String {
    format!("{level},ort=warn,xcap=warn,hyper=warn,reqwest=warn,tao=warn,wry=warn,redb=warn")
}

/// 初始化日志：写入 Tauri 提供的应用日志目录，按天滚动并保留最近 14 天。
pub fn init(log_dir: &Path) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(directives("info")));
    let (filter, handle) = reload::Layer::new(filter);
    let console = tracing_subscriber::fmt::layer().compact();
    let file = std::fs::create_dir_all(log_dir).map_err(|e| e.to_string()).and_then(|_| {
        RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("runtime")
            .filename_suffix("log")
            .max_log_files(14)
            .build(log_dir)
            .map_err(|e| e.to_string())
    });
    match file {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let _ = GUARD.set(guard);
            let file_layer = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer).compact();
            let _ = tracing_subscriber::registry().with(filter).with(console).with(file_layer).try_init();
        }
        Err(e) => {
            let _ = tracing_subscriber::registry().with(filter).with(console).try_init();
            tracing::warn!("无法写入日志目录 {}：{e}", log_dir.display());
        }
    }
    let _ = FILTER.set(handle);
}

/// 按配置调整日志级别；设置了 RUST_LOG 时以环境变量为准。
pub fn apply_level(level: &str) {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }
    if let Some(handle) = FILTER.get() {
        let _ = handle.reload(EnvFilter::new(directives(level)));
    }
}
