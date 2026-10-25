//! 开发桥：`POST /invoke/<命令>` 调用与 Tauri 命令同名的服务方法，`GET /events` 以服务器推送事件转发服务事件。
//! 屏幕识别使用模拟输入，`POST /invoke/dev_show_board` 设置模拟画面。
//!
//! 用法：`pnpm bridge --data <数据目录>`（默认端口 1422），再运行 `pnpm dev` 并在浏览器打开 http://localhost:1420。

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;
use tiny_http::Header;
use tiny_http::Method;
use tiny_http::Response;
use tiny_http::Server;
use xq_app::ApiError;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::capture::SimBackend;
use xq_app::events::EventSink;
use xq_core::fen;
use xq_core::study::Orientation;

#[derive(Default)]
struct Broadcast {
    clients: Mutex<Vec<mpsc::Sender<String>>>,
}

impl EventSink for Broadcast {
    fn emit(&self, event: &str, payload: Value) {
        let message = format!("data: {}\n\n", json!({ "event": event, "payload": payload }));
        if let Ok(mut clients) = self.clients.lock() {
            clients.retain(|c| c.send(message.clone()).is_ok());
        }
    }
}

/// 在原始连接上输出服务器推送事件：每条事件立即刷新，空闲时发送注释行保持连接。
/// （tiny_http 的分块响应会缓冲到 8 KB 才发送，不适合事件流。）
fn stream_events(mut out: Box<dyn Write + Send>, rx: mpsc::Receiver<String>) {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";
    if out.write_all(head.as_bytes()).and_then(|_| out.flush()).is_err() {
        return;
    }
    loop {
        let message = match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => ": keep-alive\n\n".to_string(),
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if out.write_all(message.as_bytes()).and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, ApiError> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|e| ApiError::from(format!("参数 {name} 无效：{e}")))
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, ApiError> {
    serde_json::to_value(v).map_err(|e| ApiError::from(e.to_string()))
}

fn dispatch(service: &AppService, sim: &SimBackend, cmd: &str, args: &Value) -> Result<Value, ApiError> {
    match cmd {
        "startup_error" => Ok(Value::Null),
        "app_status" => to_value(service.status()),
        "update_config" => to_value(service.update_config(arg(args, "config")?)?),
        "restart_engine" => {
            service.restart_engine();
            Ok(Value::Null)
        }
        "list_studies" => to_value(service.list_studies()?),
        "create_study" => to_value(service.create_study(arg(args, "req")?)?),
        "validate_position" => to_value(service.validate_position(arg(args, "req")?)),
        "load_study" => to_value(service.load_study(&arg::<String>(args, "id")?)?),
        "study_view" => to_value(service.study_view(&arg::<String>(args, "id")?)?),
        "update_study" => to_value(service.update_study(arg(args, "req")?)?),
        "delete_study" => to_value(service.delete_study(&arg::<String>(args, "id")?)?),
        "node_detail" => to_value(service.node_detail(arg(args, "req")?)?),
        "apply_move" => to_value(service.apply_move(arg(args, "req")?)?),
        "add_line" => to_value(service.add_line(arg(args, "req")?)?),
        "delete_node" => to_value(service.delete_node(arg(args, "req")?)?),
        "promote_node" => to_value(service.promote_node(arg(args, "req")?)?),
        "set_note" => to_value(service.set_note(arg(args, "req")?)?),
        "record_practice" => to_value(service.record_practice(arg(args, "req")?)?),
        "focus_node" => {
            service.focus_node(arg(args, "req")?);
            Ok(Value::Null)
        }
        "explain_move" => to_value(service.explain_move(arg(args, "req")?)?),
        "coach_explain" => to_value(service.coach_explain(arg(args, "req")?)?),
        "coach_cancel" => to_value(service.coach_cancel(arg(args, "req")?)?),
        "coach_leave" => to_value(service.coach_leave(arg(args, "req")?)?),
        "coach_view" => to_value(service.coach_view(arg(args, "req")?)?),
        "set_coach_credential" => to_value(service.set_coach_credential(arg(args, "req")?)?),
        "analyze_position" => to_value(service.analyze_position(arg(args, "req")?)?),
        "compare_moves" => to_value(service.compare_moves(arg(args, "req")?)?),
        "cancel_analysis" => {
            service.cancel_analysis(&arg::<String>(args, "requestId")?);
            Ok(Value::Null)
        }
        "cloud_lookup" => to_value(service.cloud_lookup(arg(args, "req")?)?),
        "export_study" => to_value(service.export_study(&arg::<String>(args, "id")?)?),
        "import_text" => to_value(service.import_text(&arg::<String>(args, "text")?, arg(args, "name")?)?),
        "diagnostics" => to_value(service.diagnostics()),
        "open_log_dir" => Err("开发桥不支持打开目录".into()),
        "list_windows" => to_value(service.list_windows()?),
        "capture_snapshot" => to_value(service.capture_snapshot(arg(args, "windowId")?)?),
        "start_capture" => to_value(service.start_capture(arg(args, "req")?)?),
        "stop_capture" => to_value(service.stop_capture()),
        "confirm_capture_start" => to_value(service.confirm_capture_start(arg(args, "req")?)?),
        "resolve_capture" => to_value(service.resolve_capture(arg(args, "req")?)?),
        "relocate_capture" => to_value(service.relocate_capture()?),
        "dev_show_board" => {
            let board: Option<String> = arg(args, "board")?;
            let orientation: Orientation = arg(args, "orientation")?;
            let board = match board {
                Some(b) => Some(fen::parse_fen(&b).map_err(|e| e.to_string())?.board),
                None => None,
            };
            sim.show(board, orientation);
            Ok(Value::Null)
        }
        other => Err(format!("未知命令 {other}").into()),
    }
}

fn header(name: &str, value: &str) -> Header { Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("固定的响应头") }

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let mut port = 1422u16;
    let mut data = std::env::temp_dir().join("xqlink-dev");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or(port),
            "--data" => data = args.next().map(PathBuf::from).unwrap_or(data),
            _ => {}
        }
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let paths = AppPaths {
        data_dir: data.join("data"),
        config_dir: data.join("config"),
        log_dir: data.join("logs"),
        resource_dir: repo,
        legacy_config: None,
    };
    let sink = Arc::new(Broadcast::default());
    let sim = Arc::new(SimBackend::new());
    let service = match AppService::start(paths, sink.clone(), sim.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("服务启动失败：{e}");
            std::process::exit(1);
        }
    };
    let server = match Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("无法监听端口 {port}：{e}");
            std::process::exit(1);
        }
    };
    println!("开发桥已启动：http://127.0.0.1:{port}（数据目录 {}）", data.display());
    for mut request in server.incoming_requests() {
        let (service, sim, sink) = (service.clone(), sim.clone(), sink.clone());
        std::thread::spawn(move || {
            let url = request.url().to_string();
            if request.method() == &Method::Get && url == "/events" {
                let (tx, rx) = mpsc::channel();
                let _ = tx.send(": connected\n\n".to_string());
                if let Ok(mut clients) = sink.clients.lock() {
                    clients.push(tx);
                }
                stream_events(request.into_writer(), rx);
                return;
            }
            let Some(cmd) = url.strip_prefix("/invoke/").map(str::to_string) else {
                let _ = request.respond(Response::from_string("not found").with_status_code(404));
                return;
            };
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let args: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let (status, payload) = match dispatch(&service, &sim, &cmd, &args) {
                Ok(v) => (200, v),
                Err(e) => (400, json!({ "message": e.message })),
            };
            let response = Response::from_string(payload.to_string())
                .with_status_code(status)
                .with_header(header("Content-Type", "application/json; charset=utf-8"));
            let _ = request.respond(response);
        });
    }
}
