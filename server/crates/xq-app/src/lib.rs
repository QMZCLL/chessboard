//! 学习助手的应用服务层。
//!
//! 与界面框架无关：Tauri 命令和开发用的 HTTP 桥都调用这里的 `AppService`。
//! 屏幕捕获通过 `capture::CaptureBackend` 注入，具体实现位于 Tauri 程序中。

pub mod api;
pub mod archive;
pub mod capture;
mod capture_glue;
pub mod compare;
pub mod config;
pub mod decision;
pub mod engine;
pub mod events;
pub mod llm;
mod pipeline;
pub mod records;
pub mod review;
pub mod schedule;
mod service;
pub mod storage;
mod transfer;

pub use service::APP_VERSION;
pub use service::ApiError;
pub use service::ApiResult;
pub use service::AppPaths;
pub use service::AppService;
pub use service::CommitHook;
