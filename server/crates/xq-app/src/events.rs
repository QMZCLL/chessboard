//! 推送给界面的事件。分析与讲解结果都携带学习会话、节点与请求编号，
//! 界面据此决定更新哪个节点，过期结果不会覆盖用户正在查看的内容。

use serde::Serialize;

pub const ENGINE_STATUS: &str = "engine_status";
pub const ANALYSIS_PROGRESS: &str = "analysis_progress";
pub const ANALYSIS_DONE: &str = "analysis_done";
pub const EXPLANATION_UPDATED: &str = "explanation_updated";
pub const STUDY_UPDATED: &str = "study_updated";
pub const CAPTURE_STATE: &str = "capture_state";
pub const CLOUD_RESULT: &str = "cloud_result";

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// 不推送任何事件（测试用）。
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: &str, _payload: serde_json::Value) {}
}

pub fn emit<T: Serialize>(sink: &dyn EventSink, event: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(value) => sink.emit(event, value),
        Err(e) => tracing::error!("事件 {event} 序列化失败：{e}"),
    }
}
