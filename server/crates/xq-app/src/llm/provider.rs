//! 模型服务适配。OpenAI、DeepSeek 与 Ollama 的结构化输出参数不同，分别构造请求、解析响应：
//!
//! - OpenAI：Chat Completions，`response_format` 为严格的 JSON Schema；
//! - DeepSeek：OpenAI 兼容接口，`response_format` 为 `json_object`，需要处理空内容与截断；
//! - Ollama：本机 `/api/chat`，在 `format` 中提供 JSON Schema。

use std::time::Duration;

use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::config::CoachConfig;
use crate::config::CoachProvider;
use crate::service::APP_VERSION;

#[derive(Clone)]
pub struct ProviderSettings {
    pub provider: CoachProvider,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
    pub timeout: Duration,
    pub max_tokens: u32,
}

impl std::fmt::Debug for ProviderSettings {
    // 不在日志或错误信息中输出密钥
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSettings")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("key", &self.key.as_ref().map(|_| "<已配置>"))
            .finish()
    }
}

impl ProviderSettings {
    pub fn from_config(cfg: &CoachConfig, key: Option<String>) -> Self {
        Self {
            provider: cfg.provider,
            base_url: cfg.base_url(),
            model: cfg.model.clone(),
            key,
            timeout: Duration::from_millis(cfg.timeout_ms),
            max_tokens: cfg.max_output_tokens,
        }
    }

    /// 模型配置版本：接口、地址、模型与输出上限（不含密钥）。
    pub fn revision(&self) -> String { model_revision(self.provider, &self.base_url, &self.model, self.max_tokens) }

    pub fn label(&self) -> String { format!("{} · {}", self.provider.label(), self.model) }
}

pub fn model_revision(provider: CoachProvider, base_url: &str, model: &str, max_tokens: u32) -> String {
    let text = format!("{provider:?}|{base_url}|{model}|{max_tokens}");
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
}

impl Message {
    pub fn system(content: &str) -> Self { Self { role: "system", content: content.to_string() } }

    pub fn user(content: String) -> Self { Self { role: "user", content } }

    pub fn assistant(content: String) -> Self { Self { role: "assistant", content } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderError {
    Timeout,
    Network(String),
    Http { status: u16, detail: String },
    Empty,
    Truncated,
    Refused(String),
    Malformed(String),
}

impl ProviderError {
    /// 网络问题、限流与服务端错误可以稍后重试；其他错误重试也不会改变结果。
    pub fn retryable(&self) -> bool {
        match self {
            ProviderError::Timeout | ProviderError::Network(_) => true,
            ProviderError::Http { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Timeout => write!(f, "模型服务响应超时"),
            ProviderError::Network(e) => write!(f, "无法连接模型服务：{e}"),
            ProviderError::Http { status: status @ (401 | 403), detail } => {
                write!(f, "模型服务拒绝了请求，请检查密钥（HTTP {status}）：{detail}")
            }
            ProviderError::Http { status: 404, detail } => write!(f, "找不到模型服务或模型，请检查服务地址与模型名称：{detail}"),
            ProviderError::Http { status, detail } => write!(f, "模型服务返回 HTTP {status}：{detail}"),
            ProviderError::Empty => write!(f, "模型返回了空内容"),
            ProviderError::Truncated => write!(f, "模型输出被截断，请调大“最大输出长度”或改用简短讲解"),
            ProviderError::Refused(r) => write!(f, "模型拒绝回答：{r}"),
            ProviderError::Malformed(e) => write!(f, "无法解析模型服务的响应：{e}"),
        }
    }
}

/// 构造请求地址与请求体。
pub fn request(settings: &ProviderSettings, messages: &[Message], schema: &Value) -> (String, Value) {
    let msgs: Vec<Value> = messages.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect();
    match settings.provider {
        CoachProvider::Openai => (
            format!("{}/chat/completions", settings.base_url),
            json!({
                "model": settings.model,
                "messages": msgs,
                "max_completion_tokens": settings.max_tokens,
                "response_format": {
                    "type": "json_schema",
                    "json_schema": { "name": "coach_explanation", "strict": true, "schema": schema },
                },
            }),
        ),
        CoachProvider::Deepseek => (
            format!("{}/chat/completions", settings.base_url),
            json!({
                "model": settings.model,
                "messages": msgs,
                "max_tokens": settings.max_tokens,
                "response_format": { "type": "json_object" },
                "stream": false,
            }),
        ),
        CoachProvider::Ollama => (
            format!("{}/api/chat", settings.base_url),
            json!({
                "model": settings.model,
                "messages": msgs,
                "stream": false,
                "format": schema,
                "options": { "num_predict": settings.max_tokens },
            }),
        ),
    }
}

/// 从响应中取出模型输出的文字。
pub fn parse_response(provider: CoachProvider, body: &str) -> Result<String, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|e| ProviderError::Malformed(e.to_string()))?;
    let (content, finish) = match provider {
        CoachProvider::Openai | CoachProvider::Deepseek => {
            let choice = value.get("choices").and_then(|c| c.get(0)).ok_or(ProviderError::Malformed("缺少 choices".into()))?;
            let message = &choice["message"];
            if let Some(refusal) = message.get("refusal").and_then(Value::as_str).filter(|r| !r.is_empty()) {
                return Err(ProviderError::Refused(refusal.to_string()));
            }
            (message.get("content").and_then(Value::as_str), choice.get("finish_reason").and_then(Value::as_str))
        }
        CoachProvider::Ollama => {
            (value["message"].get("content").and_then(Value::as_str), value.get("done_reason").and_then(Value::as_str))
        }
    };
    if finish == Some("length") {
        return Err(ProviderError::Truncated);
    }
    match content.map(str::trim) {
        Some(text) if !text.is_empty() => Ok(text.to_string()),
        _ => Err(ProviderError::Empty),
    }
}

fn shorten(text: &str) -> String {
    let t: String = text.chars().take(200).collect();
    if text.chars().count() > 200 { format!("{t}…") } else { t }
}

/// 服务地址是否在本机（127.0.0.1、::1、localhost）。
pub fn is_local(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url) else { return false };
    let Some(host) = url.host_str() else { return false };
    let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host == "localhost" || host.ends_with(".localhost"),
    }
}

/// 发送一次请求，返回模型输出的文字。
pub fn complete(settings: &ProviderSettings, messages: &[Message], schema: &Value) -> Result<String, ProviderError> {
    let mut builder = reqwest::blocking::Client::builder().timeout(settings.timeout).user_agent(format!("xqlink/{APP_VERSION}"));
    // 云端服务按系统代理设置访问；本机服务（如 Ollama）不经过代理：代理通常只用来访问外网，
    // 经代理转发到本机地址会失败或绕路
    if is_local(&settings.base_url) {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|e| ProviderError::Network(e.to_string()))?;
    let (url, body) = request(settings, messages, schema);
    let mut req = client.post(&url).header("Content-Type", "application/json").body(body.to_string());
    if let Some(key) = &settings.key {
        req = req.bearer_auth(key);
    }
    let response = req.send().map_err(|e| {
        if e.is_timeout() { ProviderError::Timeout } else { ProviderError::Network(e.without_url().to_string()) }
    })?;
    let status = response.status();
    let text = response.text().map_err(|e| {
        if e.is_timeout() { ProviderError::Timeout } else { ProviderError::Network(e.without_url().to_string()) }
    })?;
    if !status.is_success() {
        return Err(ProviderError::Http { status: status.as_u16(), detail: shorten(&text) });
    }
    parse_response(settings.provider, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(provider: CoachProvider) -> ProviderSettings {
        ProviderSettings {
            provider,
            base_url: provider.default_base_url().into(),
            model: "m".into(),
            key: Some("secret".into()),
            timeout: Duration::from_secs(1),
            max_tokens: 900,
        }
    }

    #[test]
    fn each_provider_gets_its_own_structured_output_parameters() {
        let schema = json!({ "type": "object" });
        let msgs = [Message::system("s"), Message::user("u".into())];
        let (url, body) = request(&settings(CoachProvider::Openai), &msgs, &schema);
        assert!(url.ends_with("/v1/chat/completions"));
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["max_completion_tokens"], 900);
        let (url, body) = request(&settings(CoachProvider::Deepseek), &msgs, &schema);
        assert_eq!(url, "https://api.deepseek.com/chat/completions");
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["max_tokens"], 900);
        let (url, body) = request(&settings(CoachProvider::Ollama), &msgs, &schema);
        assert!(url.ends_with("/api/chat"));
        assert_eq!(body["format"], schema);
        assert_eq!(body["stream"], false);
        // 请求体与调试输出都不含密钥
        assert!(!body.to_string().contains("secret"));
        assert!(!format!("{:?}", settings(CoachProvider::Openai)).contains("secret"));
    }

    #[test]
    fn local_services_are_recognized() {
        for url in ["http://127.0.0.1:11434", "http://localhost:11434", "http://[::1]:8080/v1", "http://api.localhost"] {
            assert!(is_local(url), "{url}");
        }
        for url in ["https://api.deepseek.com", "https://api.openai.com/v1", "http://192.168.1.5:11434", "not a url"] {
            assert!(!is_local(url), "{url}");
        }
    }

    #[test]
    fn empty_truncated_and_refused_responses() {
        let ok = r#"{"choices":[{"message":{"content":" {\"a\":1} "},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_response(CoachProvider::Deepseek, ok).unwrap(), "{\"a\":1}");
        let empty = r#"{"choices":[{"message":{"content":""},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_response(CoachProvider::Deepseek, empty), Err(ProviderError::Empty));
        let cut = r#"{"choices":[{"message":{"content":"{\"a\":"},"finish_reason":"length"}]}"#;
        assert_eq!(parse_response(CoachProvider::Openai, cut), Err(ProviderError::Truncated));
        let refused = r#"{"choices":[{"message":{"content":null,"refusal":"不能回答"},"finish_reason":"stop"}]}"#;
        assert!(matches!(parse_response(CoachProvider::Openai, refused), Err(ProviderError::Refused(_))));
        let ollama = r#"{"message":{"role":"assistant","content":"{}"},"done":true,"done_reason":"stop"}"#;
        assert_eq!(parse_response(CoachProvider::Ollama, ollama).unwrap(), "{}");
        let ollama_cut = r#"{"message":{"content":"{"},"done":true,"done_reason":"length"}"#;
        assert_eq!(parse_response(CoachProvider::Ollama, ollama_cut), Err(ProviderError::Truncated));
        assert!(matches!(parse_response(CoachProvider::Openai, "<html>"), Err(ProviderError::Malformed(_))));
        assert!(ProviderError::Http { status: 503, detail: String::new() }.retryable());
        assert!(!ProviderError::Http { status: 401, detail: String::new() }.retryable());
    }
}
