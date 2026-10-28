//! AI 讲解流程：模拟引擎 + 模拟语言模型服务（本机 HTTP）。
//!
//! 覆盖：密钥不外露、缓存复用、失败回退、等待引擎分析、证据更新后旧结果不写入、取消、
//! 关闭功能或更换设置后不再发送排队中的请求、重启后清理中断的状态、状态版本。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::api::*;
use xq_app::capture::SimBackend;
use xq_app::compare::CompareRequest;
use xq_app::config::Audience;
use xq_app::config::CoachProvider;
use xq_app::engine::EngineState;
use xq_app::events::EventSink;
use xq_app::llm::CoachStatus;
use xq_app::llm::ask::AnswersRequest;
use xq_app::llm::ask::Question;
use xq_app::llm::ask::QuestionKind;
use xq_app::records::Budget;
use xq_core::Move;
use xq_core::study::NodeId;
use xq_core::study::ROOT;

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(String, Value)>>,
    signal: Condvar,
}

impl EventSink for Recorder {
    fn emit(&self, event: &str, payload: Value) {
        self.events.lock().unwrap().push((event.to_string(), payload));
        self.signal.notify_all();
    }
}

impl Recorder {
    fn wait(&self, what: &str, pred: impl Fn(&str, &Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut events = self.events.lock().unwrap();
        loop {
            if let Some((_, v)) = events.iter().find(|(e, v)| pred(e, v)) {
                return v.clone();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "等待事件超时：{what}");
            events = self.signal.wait_timeout(events, left).unwrap().0;
        }
    }

    fn clear(&self) { self.events.lock().unwrap().clear(); }

    fn coach(&self, what: &str, node: NodeId, status: &str) -> Value {
        self.wait(what, |e, v| e == "coach_updated" && v["node_id"] == node && v["status"] == status)
    }
}

fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

/// 模拟语言模型服务的一次回复。
enum Reply {
    /// 正常返回（DeepSeek 格式），内容由函数根据请求生成。
    Content(fn(&Value) -> String),
    Raw(u16, String),
}

/// 收到的请求：(Authorization 头, 请求体)。
type Calls = Arc<Mutex<Vec<(Option<String>, Value)>>>;

struct MockLlm {
    url: String,
    calls: Calls,
}

impl MockLlm {
    fn count(&self) -> usize { self.calls.lock().unwrap().len() }

    fn call(&self, i: usize) -> (Option<String>, Value) { self.calls.lock().unwrap()[i].clone() }

    fn wait_calls(&self, n: usize) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.count() < n {
            assert!(Instant::now() < deadline, "模型服务没有收到第 {n} 次请求");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// `script` 给出第 i 次请求的回复与延迟（毫秒）；超出脚本时返回 HTTP 500。
#[test]
fn one_move_can_take_every_retry_and_each_call_counts_against_the_budget() {
    // 服务端错误后网络重试成功但输出不合格；修正请求又遇到服务端错误，重试后得到合格输出
    let llm = mock_llm(|i| match i {
        0 | 2 => (Reply::Raw(503, r#"{"error":{"message":"busy"}}"#.into()), 0),
        1 => (Reply::Content(unknown_claim), 0),
        _ => (Reply::Content(valid_output), 0),
    });
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), &format!("@go\n{ROUND}"), &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk".into()) }).unwrap();
    let policy = service.status().coach.calls_per_move;
    assert_eq!((policy.usual, policy.with_repair, policy.max), (1, 2, 4), "界面说明的次数与重试策略一致");
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");
    explain(&service, &sid, node, false);
    rec.coach("经过全部重试后生成", node, "ready");
    assert_eq!(llm.count(), policy.max as usize, "一着最多调用的次数");
    assert_eq!(service.status().coach.used_today, policy.max, "每次调用都计入每日次数");
    service.shutdown();
}

fn mock_llm(script: impl Fn(usize) -> (Reply, u64) + Send + Sync + 'static) -> MockLlm {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    let calls: Calls = Arc::default();
    let log = calls.clone();
    let script = Arc::new(script);
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let auth = request.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.to_string());
            let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let index = {
                let mut log = log.lock().unwrap();
                log.push((auth, body.clone()));
                log.len() - 1
            };
            let script = script.clone();
            std::thread::spawn(move || {
                let (reply, delay) = script(index);
                std::thread::sleep(Duration::from_millis(delay));
                let (status, text) = match reply {
                    Reply::Content(make) => (200, chat(&make(&body))),
                    Reply::Raw(status, text) => (status, text),
                };
                let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let _ = request.respond(tiny_http::Response::from_string(text).with_status_code(status).with_header(header));
            });
        }
    });
    MockLlm { url, calls }
}

fn chat(content: &str) -> String {
    json!({ "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }] }).to_string()
}

/// 请求中的证据包。
fn pack(body: &Value) -> Value {
    let user = body["messages"][1]["content"].as_str().unwrap();
    serde_json::from_str::<Value>(user).unwrap()["evidence"].clone()
}

fn claim(pack: &Value, groups: &[&str]) -> String {
    let claims = pack["claims"].as_array().unwrap();
    claims.iter().find(|c| groups.contains(&c["group"].as_str().unwrap())).unwrap()["id"].as_str().unwrap().to_string()
}

/// 只使用证据包中的结论编号、不含着法与数字的合格讲解。
fn valid_output(body: &Value) -> String {
    let p = pack(body);
    json!({
        "summary": { "text": "这步让炮来到中路，加强对中路的控制。", "claim_ids": [claim(&p, &["basis"])] },
        "sections": [
            { "kind": "purpose", "text": "当前没有直接威胁，可以着手出动子力。", "claim_ids": [claim(&p, &["need"])] },
            { "kind": "comparison", "text": "候选比较的结论见下方程序给出的评价。", "claim_ids": [claim(&p, &["candidate", "comparison"])] },
        ],
        "lesson": { "text": "出子时同时检查新增的攻击和保护关系。", "claim_ids": [claim(&p, &["lesson"])] },
        "followups": [{ "action": "compare_alternative", "label": "看另一个候选" }],
    })
    .to_string()
}

/// 回答追问：引用与问题相符的结论，不含着法以外的数字与硬结论。
fn valid_answer(body: &Value) -> String {
    let p = pack(body);
    let group = match p["question"]["kind"].as_str().unwrap() {
        "my_move" => "mine",
        "difference" => "difference",
        _ => "mine_reply",
    };
    json!({
        "answer": { "text": "这步让炮来到中路，接下来要留意对方出马保护中卒。", "claim_ids": [claim(&p, &[group])] },
        "points": [],
        "check": "中卒现在由哪些棋子保护？",
    })
    .to_string()
}

fn bad_answer(_: &Value) -> String {
    json!({ "answer": { "text": "这步更好。", "claim_ids": [] }, "points": [], "check": "" }).to_string()
}

fn unknown_claim(_: &Value) -> String {
    json!({
        "summary": { "text": "出动炮。", "claim_ids": ["c999"] },
        "sections": [{ "kind": "purpose", "text": "出动炮。", "claim_ids": ["c999"] }],
        "lesson": { "text": "", "claim_ids": [] },
        "followups": [],
    })
    .to_string()
}

const ROUND: &str = "info depth 1 seldepth 1 multipv 1 score cp 30 nodes 10 time 1 pv h2e2 h9g7\n\
    info depth 1 seldepth 1 multipv 2 score cp 20 nodes 10 time 1 pv h0g2\n\
    bestmove h2e2\n";
/// 炮二平五之后黑方的候选。
const BLACK_ROUND: &str = "info depth 1 seldepth 1 multipv 1 score cp 10 nodes 10 time 1 pv h9g7 h0g2\n\
    info depth 1 seldepth 1 multipv 2 score cp 5 nodes 10 time 1 pv b9c7\n\
    bestmove h9g7\n";

fn repo_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..") }

fn start(dir: &std::path::Path, engine_script: &str, llm: &MockLlm, rec: Arc<Recorder>) -> AppService {
    start_with(dir, engine_script, llm, rec, |_| {})
}

fn start_with(
    dir: &std::path::Path, engine_script: &str, llm: &MockLlm, rec: Arc<Recorder>,
    tweak: impl FnOnce(&mut xq_app::config::AppConfig),
) -> AppService {
    let engine_dir = dir.join("engine");
    std::fs::create_dir_all(&engine_dir).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_xq-mock-uci"));
    let binary = engine_dir.join(exe.file_name().unwrap());
    std::fs::copy(&exe, &binary).unwrap();
    std::fs::write(engine_dir.join("mock-uci-script.txt"), engine_script).unwrap();
    let mut cfg = xq_app::config::AppConfig::default();
    cfg.engine.binary = Some(binary);
    cfg.analysis.multipv = 2;
    cfg.coach.enabled = true;
    cfg.coach.provider = CoachProvider::Deepseek;
    cfg.coach.base_url = llm.url.clone();
    cfg.coach.model = "mock-model".into();
    cfg.coach.timeout_ms = 2_000;
    tweak(&mut cfg);
    xq_app::config::save(&dir.join("config").join("config.json"), &cfg).unwrap();
    let paths = AppPaths {
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        log_dir: dir.join("logs"),
        resource_dir: repo_root(),
        legacy_config: None,
    };
    let service = AppService::start(paths, rec, Arc::new(SimBackend::new())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !matches!(service.status().engine, EngineState::Ready { .. }) {
        assert!(Instant::now() < deadline, "{:?}", service.status().engine);
        std::thread::sleep(Duration::from_millis(20));
    }
    service
}

fn standard_study(service: &AppService) -> String {
    service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap()
        .study
        .study
        .id
}

fn explain(service: &AppService, sid: &str, node: NodeId, refresh: bool) -> xq_app::llm::CoachView {
    service.coach_explain(CoachExplainRequest { study_id: sid.into(), node_id: node, audience: None, detail: None, refresh }).unwrap()
}

#[test]
fn coach_explains_from_evidence_reuses_cache_and_keeps_key_private() {
    let llm = mock_llm(|_| (Reply::Content(valid_output), 0));
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let deep = ROUND.replace("cp 30", "cp 45");
    let service = start(dir.path(), &format!("@go\n{ROUND}@go\n{deep}"), &llm, rec.clone());
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");

    // 没有密钥：不可用，不发出请求，基本讲解照常
    let view = explain(&service, &sid, node, false);
    assert_eq!(view.status, CoachStatus::Disabled);
    assert!(view.reason.unwrap().contains("密钥"));

    // 保存密钥：配置只记录引用，状态只说明已配置
    let status = service.set_coach_credential(CoachCredentialRequest { key: Some("sk-test-secret".into()) }).unwrap();
    assert!(status.coach.key_configured && status.coach.ready, "{:?}", status.coach);
    assert_eq!(status.config.coach.credential_ref.as_deref(), Some("store:coach"));
    assert!(!serde_json::to_string(&status).unwrap().contains("sk-test-secret"));
    assert!(!std::fs::read_to_string(dir.path().join("config/config.json")).unwrap().contains("sk-test-secret"));

    rec.clear();
    explain(&service, &sid, node, false);
    let ready = rec.coach("AI 讲解完成", node, "ready");
    assert_eq!(llm.count(), 1);
    let (auth, body) = llm.call(0);
    assert_eq!(auth.as_deref(), Some("Bearer sk-test-secret"));
    assert_eq!(body["response_format"]["type"], "json_object");
    let p = pack(&body);
    assert_eq!(p["move_notation"], "炮二平五");
    assert_eq!(p["analysis"]["level"], "detailed");
    assert!(p["score_perspective"].as_str().unwrap().contains("红方"));
    // 证据包不含学习会话编号等元数据，这些由后台附加
    assert!(!body.to_string().contains(&sid));
    let result = &ready["record"]["result"];
    assert_eq!(result["output"]["summary"]["text"], "这步让炮来到中路，加强对中路的控制。");
    assert!(!result["claims"].as_array().unwrap().is_empty());
    let followup = &result["output"]["followups"][0];
    assert!(followup["evidence_id"].is_string(), "{followup}");
    let evidence_ids: Vec<&str> = result["evidence"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert!(evidence_ids.contains(&followup["evidence_id"].as_str().unwrap()), "后续操作对应的回放证据随结果保存");
    // AI 讲解成功不改变分析状态
    let detail = service.node_detail(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    assert_eq!(detail.explanation.unwrap().status(), xq_app::records::ExplainStatus::Detailed);
    assert_eq!(detail.coach.unwrap().status, CoachStatus::Ready);

    // 再次打开同一着：复用结果，不再调用模型
    rec.clear();
    explain(&service, &sid, node, false);
    rec.coach("复用", node, "ready");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(llm.count(), 1);
    // 明确要求重新生成
    rec.clear();
    explain(&service, &sid, node, true);
    llm.wait_calls(2);
    rec.coach("重新生成完成", node, "ready");
    assert_eq!(service.status().coach.used_today, 2);

    // 状态版本：证据更新后推送的“已过期”版本更大，较早读到的“已生成”不会被界面当作更新的状态
    let earlier = service.coach_view(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    assert_eq!(earlier.status, CoachStatus::Ready);
    rec.clear();
    service.explain_move(ExplainRequest { study_id: sid.clone(), node_id: node, budget: Budget::Deep, retry: false }).unwrap();
    let stale = rec.coach("证据更新后过期", node, "stale");
    assert!(stale["seq"].as_u64().unwrap() > earlier.seq, "{} <= {}", stale["seq"], earlier.seq);
    assert!(stale["reason"].as_str().unwrap().contains("依据已更新"));
    service.shutdown();
}

#[test]
fn coach_failures_fall_back_to_template_explanation() {
    let llm = mock_llm(|i| match i {
        // 引用不存在的编号：第一次与修正后都不合格
        0 | 1 => (Reply::Content(unknown_claim), 0),
        2 => (Reply::Raw(200, r#"{"choices":[{"message":{"content":"{\"summary\":"},"finish_reason":"length"}]}"#.into()), 0),
        3 => (Reply::Raw(200, r#"{"choices":[{"message":{"content":""},"finish_reason":"stop"}]}"#.into()), 0),
        4 => (Reply::Raw(401, r#"{"error":{"message":"invalid api key"}}"#.into()), 0),
        // 超时两次（第一次超时后重试一次）
        5 | 6 => (Reply::Content(valid_output), 2_600),
        _ => (Reply::Content(valid_output), 0),
    });
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), &format!("@go\n{ROUND}"), &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk".into()) }).unwrap();
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");

    let cases = [
        (2, "没有通过检查"),
        (3, "截断"),
        (4, "空内容"),
        (5, "请检查密钥"),
        (7, "超时"),
    ];
    for (calls, reason) in cases {
        rec.clear();
        explain(&service, &sid, node, true);
        let failed = rec.coach(reason, node, "failed");
        assert!(failed["reason"].as_str().unwrap().contains(reason), "{failed}");
        assert_eq!(llm.count(), calls, "{reason}");
        // 模板讲解不受影响
        let record = service.node_detail(NodeRef { study_id: sid.clone(), node_id: node }).unwrap().explanation.unwrap();
        assert_eq!(record.status(), xq_app::records::ExplainStatus::Detailed);
    }
    // 服务恢复后可以重新生成
    rec.clear();
    explain(&service, &sid, node, true);
    rec.coach("恢复", node, "ready");
    service.shutdown();
}

#[test]
fn coach_waits_for_engine_and_never_commits_outdated_evidence() {
    // 第 1 次（快速）搜索慢一些，让 AI 讲解先等待；第 2 次是深入分析
    let deep = ROUND.replace("cp 30", "cp 45");
    let engine = format!("@go\nsleep 600\n{ROUND}@go\n{deep}");
    // 第 1 次模型请求较慢：返回前证据已经更新
    let llm = mock_llm(|i| (Reply::Content(valid_output), if i == 0 { 1_200 } else if i == 2 { 1_500 } else { 0 }));
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), &engine, &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk".into()) }).unwrap();
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    explain(&service, &sid, node, false);
    let queued = rec.coach("等待引擎", node, "queued");
    assert!(queued["reason"].as_str().unwrap().contains("等待引擎"), "{queued}");
    assert_eq!(llm.count(), 0, "引擎分析完成前不调用模型");

    // 引擎完成后开始生成；生成期间要求深入分析，证据随之更新
    llm.wait_calls(1);
    assert_eq!(pack(&llm.call(0).1)["analysis"]["level"], "detailed");
    service.explain_move(ExplainRequest { study_id: sid.clone(), node_id: node, budget: Budget::Deep, retry: false }).unwrap();
    rec.wait("深入讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["record"]["budget"] == "deep");
    // 旧证据的结果不写入，按新证据重新生成
    let ready = rec.coach("按新证据生成", node, "ready");
    assert_eq!(llm.count(), 2);
    let second = pack(&llm.call(1).1);
    assert!(second["analysis"]["conditions"].as_str().unwrap().contains("深入分析"), "{second}");
    assert!(second["candidates"][0]["score"].as_str().unwrap().contains("+45"));
    let created_at = ready["record"]["result"]["created_at"].as_i64().unwrap();

    // 取消：已发出的请求返回后丢弃结果
    rec.clear();
    explain(&service, &sid, node, true);
    llm.wait_calls(3);
    service.coach_cancel(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    std::thread::sleep(Duration::from_millis(2_000));
    let view = service.coach_view(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    assert_eq!(view.status, CoachStatus::Ready, "取消后保留原结果：{:?}", view.reason);
    let record = view.record.unwrap();
    assert_eq!(record.result.unwrap().created_at, created_at, "取消后返回的结果不能写入");
    assert_eq!(serde_json::to_value(record.state).unwrap(), "idle");
    assert_eq!(llm.count(), 3);
    service.shutdown();
}

/// 等待第 1 着的请求发到模型服务；超时时把 AI 讲解与逐着讲解的状态写进失败信息，便于排查偶发问题。
fn wait_first_call(service: &AppService, llm: &MockLlm, rec: &Recorder, sid: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while llm.count() < 1 {
        if Instant::now() > deadline {
            let node = NodeRef { study_id: sid.into(), node_id: 1 };
            let view = service.coach_view(node.clone()).unwrap();
            let detail = service.node_detail(node).unwrap();
            let events: Vec<String> = rec.events.lock().unwrap().iter().rev().take(10).map(|(e, v)| format!("{e}:{}:{}", v["node_id"], v["status"])).collect();
            panic!(
                "模型服务没有收到第 1 次请求；AI 讲解 {:?}（{:?}，记录 {:?}）；逐着讲解 {:?}；最近的事件 {events:?}",
                view.status,
                view.reason,
                view.record.map(|r| (r.state, r.waiting_evidence, r.generation, r.error)),
                detail.explanation.map(|e| (e.status(), e.pending, e.error)),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 两着棋（红黑各一着）都已有详细讲解，并发数为 1；第 1 次模型请求较慢。
fn two_moves_with_slow_first_call(dir: &std::path::Path) -> (AppService, MockLlm, Arc<Recorder>, String) {
    let llm = mock_llm(|i| (Reply::Content(valid_output), if i == 0 { 1_500 } else { 0 }));
    let rec = Arc::new(Recorder::default());
    // 第 3 段供第 1 着的深入分析使用
    let deep = ROUND.replace("cp 30", "cp 45");
    let service = start_with(dir, &format!("@go\n{ROUND}@go\n{BLACK_ROUND}@go\n{deep}"), &llm, rec.clone(), |cfg| {
        cfg.coach.max_parallel_requests = 1;
    });
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk-old".into()) }).unwrap();
    let created = service
        .create_study(CreateStudyRequest {
            name: None,
            kind: CreateKind::Record { text: "1. 炮二平五 马8进7".into() },
            orientation: None,
            user_side: None,
        })
        .unwrap();
    let sid = created.study.study.id.clone();
    rec.wait("两着详细讲解", |e, v| e == "explanation_updated" && v["study_id"] == sid.as_str() && v["stats"]["detailed"] == 2);
    // 第 1 着的请求已发出且未返回；第 2 着已排队，等待并发名额
    explain(&service, &sid, 1, false);
    wait_first_call(&service, &llm, &rec, &sid);
    explain(&service, &sid, 2, false);
    rec.coach("第 2 着排队", 2, "queued");
    (service, llm, rec, sid)
}

#[test]
fn queued_requests_are_not_sent_after_ai_is_disabled_or_key_cleared() {
    type Change = fn(&AppService);
    let changes: [(&str, Change); 2] = [
        ("关闭 AI 讲解", |service| {
            let mut cfg = service.status().config;
            cfg.coach.enabled = false;
            service.update_config(cfg).unwrap();
        }),
        ("清除密钥", |service| {
            service.set_coach_credential(CoachCredentialRequest { key: None }).unwrap();
        }),
    ];
    for (what, change) in changes {
        let dir = tempfile::tempdir().unwrap();
        let (service, llm, _rec, sid) = two_moves_with_slow_first_call(dir.path());
        change(&service);
        // 第 1 着的请求返回、释放名额后，排队的第 2 着不能再发送
        std::thread::sleep(Duration::from_millis(2_500));
        assert_eq!(llm.count(), 1, "{what}：服务器只应收到已经发出的那一次请求");
        for node in [1, 2] {
            let view = service.coach_view(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
            assert_eq!(view.status, CoachStatus::Disabled, "{what}：第 {node} 着 {:?}", view.reason);
            assert!(view.record.is_none_or(|r| r.result.is_none()), "{what}：已发出请求的结果被丢弃");
        }
        service.shutdown();
    }
}

#[test]
fn queued_requests_use_new_model_after_switch() {
    let dir = tempfile::tempdir().unwrap();
    let (service, llm, rec, sid) = two_moves_with_slow_first_call(dir.path());
    let mut cfg = service.status().config;
    cfg.coach.model = "other-model".into();
    service.update_config(cfg).unwrap();
    rec.coach("第 1 着按新模型生成", 1, "ready");
    rec.coach("第 2 着按新模型生成", 2, "ready");
    // 旧模型只收到第 1 着已经发出的那次请求；之后的请求都使用新模型
    let calls: Vec<Value> = (0..llm.count()).map(|i| llm.call(i).1).collect();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert_eq!(calls[0]["model"], "mock-model");
    assert!(calls[1..].iter().all(|c| c["model"] == "other-model"));
    assert!(calls.iter().filter(|c| pack(c)["move_notation"] == "马8进7").all(|c| c["model"] == "other-model"));
    for node in [1, 2] {
        let view = service.coach_view(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
        assert!(view.record.unwrap().result.unwrap().model_label.contains("other-model"));
    }
    service.shutdown();
}

#[test]
fn interrupted_request_is_not_shown_as_running_after_restart() {
    let llm = mock_llm(|_| (Reply::Content(valid_output), 0));
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let script = format!("@go\n{ROUND}");
    let service = start(dir.path(), &script, &llm, rec.clone());
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");
    service.shutdown();
    drop(service);

    // 模拟上次退出时请求还在进行
    {
        let store = xq_app::storage::Store::open(&dir.path().join("data").join("xqlink.redb")).unwrap();
        let mut record = xq_app::llm::CoachRecord::new(&sid, node, 0);
        record.state = xq_app::llm::CoachState::Running;
        record.generation = 7;
        store.save_coach(&record).unwrap();
    }

    let service = start(dir.path(), &script, &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk".into()) }).unwrap();
    service.load_study(&sid).unwrap();
    let view = service.coach_view(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    assert_eq!(view.status, CoachStatus::None, "{:?}", view.reason);
    assert!(view.reason.unwrap().contains("没有完成"));
    assert_eq!(serde_json::to_value(view.record.unwrap().state).unwrap(), "idle");
    // 可以重新生成
    rec.clear();
    explain(&service, &sid, node, false);
    rec.coach("重新生成", node, "ready");
    assert_eq!(llm.count(), 1);
    service.shutdown();
}

#[test]
fn queued_request_is_not_sent_even_before_cleanup_runs() {
    // 讲解线程被占用时，关闭功能后的清理还没有执行；排队的请求取得名额后也必须停下
    let dir = tempfile::tempdir().unwrap();
    let (service, llm, rec, sid) = two_moves_with_slow_first_call(dir.path());
    let (paused_tx, paused_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(Some((paused_tx, resume_rx)));
    service.set_commit_hook(Some(Arc::new(move |_: &str, _, budget| {
        if budget == Budget::Deep
            && let Some((paused, resume)) = gate.lock().unwrap().take()
        {
            paused.send(()).unwrap();
            resume.recv().unwrap();
        }
    })));
    service.explain_move(ExplainRequest { study_id: sid.clone(), node_id: 1, budget: Budget::Deep, retry: false }).unwrap();
    paused_rx.recv_timeout(Duration::from_secs(30)).expect("讲解线程应停在深入讲解的提交处");
    let mut cfg = service.status().config;
    cfg.coach.enabled = false;
    service.update_config(cfg).unwrap();
    // 第 1 着的请求返回、释放名额；此时清理仍在排队
    std::thread::sleep(Duration::from_millis(2_500));
    assert_eq!(llm.count(), 1, "设置改变后，排队的请求不能用旧设置发出");
    resume_tx.send(()).unwrap();
    rec.wait("清理完成", |e, v| e == "coach_updated" && v["node_id"] == 2 && v["status"] == "disabled");
    assert_eq!(llm.count(), 1);
    service.shutdown();
}

#[test]
fn settings_read_just_before_disable_are_never_sent() {
    // 讲解线程刚读到旧设置就关闭 AI；派发恢复后、清理执行前，工作线程也不能用旧设置发出请求
    let llm = mock_llm(|_| (Reply::Content(valid_output), 0));
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), &format!("@go\n{ROUND}"), &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk".into()) }).unwrap();
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");

    let (paused_tx, paused_rx) = std::sync::mpsc::channel::<&'static str>();
    let (resume_read_tx, resume_read_rx) = std::sync::mpsc::channel::<()>();
    let (resume_sent_tx, resume_sent_rx) = std::sync::mpsc::channel::<()>();
    let gates = Mutex::new((Some(resume_read_rx), Some(resume_sent_rx)));
    service.set_coach_test_hook(Some(Arc::new(move |point: &'static str| {
        let resume = match point {
            "settings_read" => gates.lock().unwrap().0.take(),
            "job_sent" => gates.lock().unwrap().1.take(),
            _ => None,
        };
        if let Some(resume) = resume {
            paused_tx.send(point).unwrap();
            resume.recv().unwrap();
        }
    })));
    explain(&service, &sid, node, false);
    assert_eq!(paused_rx.recv_timeout(Duration::from_secs(30)).unwrap(), "settings_read");
    // 设置写入、版本递增；清理排在讲解线程上，此时讲解线程停在派发中
    let mut cfg = service.status().config;
    cfg.coach.enabled = false;
    service.update_config(cfg).unwrap();
    resume_read_tx.send(()).unwrap();
    // 派发完成、任务已交给工作线程，讲解线程再次停住，清理仍未执行
    assert_eq!(paused_rx.recv_timeout(Duration::from_secs(30)).unwrap(), "job_sent");
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(llm.count(), 0, "旧设置不能带着新版本发出");
    resume_sent_tx.send(()).unwrap();
    rec.wait("清理完成", |e, v| e == "coach_updated" && v["node_id"] == node && v["status"] == "disabled");
    assert_eq!(llm.count(), 0);
    service.shutdown();
}

/// 与界面合并 AI 讲解状态的规则相同（src/state/coachMerge.ts）：版本不小于当前时采用新到达的。
fn merge(current: Option<xq_app::llm::CoachView>, incoming: xq_app::llm::CoachView) -> xq_app::llm::CoachView {
    match current {
        Some(c) if incoming.seq < c.seq => c,
        _ => incoming,
    }
}

#[test]
fn learning_settings_change_bumps_view_version() {
    let llm = mock_llm(|_| (Reply::Content(valid_output), 0));
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), &format!("@go\n{ROUND}"), &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk".into()) }).unwrap();
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");
    explain(&service, &sid, node, false);
    rec.coach("AI 讲解完成", node, "ready");

    let node_ref = NodeRef { study_id: sid.clone(), node_id: node };
    type Change = fn(&mut xq_app::config::CoachConfig);
    let changes: [(&str, Change, CoachStatus); 3] = [
        ("学习水平改为高级", |c| c.audience = xq_app::config::Audience::Advanced, CoachStatus::Stale),
        ("学习水平改回入门", |c| c.audience = xq_app::config::Audience::Beginner, CoachStatus::Ready),
        ("详细程度改为详细", |c| c.detail = xq_app::config::Detail::Deep, CoachStatus::Stale),
    ];
    for (what, change, expected) in changes {
        let before = service.coach_view(node_ref.clone()).unwrap();
        let mut cfg = service.status().config;
        change(&mut cfg.coach);
        service.update_config(cfg).unwrap();
        let after = service.coach_view(node_ref.clone()).unwrap();
        assert_eq!(after.status, expected, "{what}：{:?}", after.reason);
        assert_ne!(before.status, after.status, "{what}");
        assert!(after.seq > before.seq, "{what}：状态改变但版本没有增加（{} → {}）", before.seq, after.seq);
        // 无论两份响应以什么顺序到达，界面最终都显示新状态
        let first = merge(Some(merge(None, before.clone())), after.clone());
        let second = merge(Some(merge(None, after.clone())), before.clone());
        assert_eq!((first.status, second.status), (expected, expected), "{what}");
    }
    assert_eq!(llm.count(), 1, "只改学习偏好不会自动调用模型");
    service.shutdown();
}

#[test]
fn failed_update_marks_kept_result_as_not_current() {
    // 第一次生成成功，之后的请求都失败
    let llm = mock_llm(|i| match i {
        0 => (Reply::Content(valid_output), 0),
        _ => (Reply::Raw(401, r#"{"error":{"message":"invalid api key"}}"#.into()), 0),
    });
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), &format!("@go\n{ROUND}"), &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk".into()) }).unwrap();
    let sid = standard_study(&service);
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");
    let node_ref = NodeRef { study_id: sid.clone(), node_id: node };
    explain(&service, &sid, node, false);
    rec.coach("AI 讲解完成", node, "ready");
    assert!(service.coach_view(node_ref.clone()).unwrap().result_current);

    // 结果仍然有效、只是重新生成失败：仍是 ready，结果对应当前条件
    rec.clear();
    explain(&service, &sid, node, true);
    rec.coach("重新生成失败", node, "ready");
    let view = service.coach_view(node_ref.clone()).unwrap();
    assert_eq!(view.status, CoachStatus::Ready);
    assert!(view.result_current && view.reason.as_deref().is_some_and(|r| r.contains("最近一次重新生成失败")), "{:?}", view.reason);

    // 设置改变后结果不再对应当前条件：最近一次请求失败，状态为失败，保留的是上次的结果
    let mut cfg = service.status().config;
    cfg.coach.audience = xq_app::config::Audience::Advanced;
    service.update_config(cfg).unwrap();
    let outdated = service.coach_view(node_ref.clone()).unwrap();
    assert_eq!(outdated.status, CoachStatus::Failed);
    assert!(outdated.record.as_ref().and_then(|r| r.result.as_ref()).is_some() && !outdated.result_current);
    // 再次更新仍然失败
    rec.clear();
    explain(&service, &sid, node, false);
    rec.coach("更新失败", node, "failed");
    let failed = service.coach_view(node_ref).unwrap();
    assert_eq!(failed.status, CoachStatus::Failed);
    assert!(failed.record.as_ref().and_then(|r| r.result.as_ref()).is_some(), "上次的结果保留");
    assert!(!failed.result_current, "保留的结果不对应当前设置，界面不能当作已生成");
    service.shutdown();
}

#[test]
fn leaving_a_move_cancels_only_unsent_requests() {
    let dir = tempfile::tempdir().unwrap();
    let (service, llm, rec, sid) = two_moves_with_slow_first_call(dir.path());
    let node = |n| NodeRef { study_id: sid.clone(), node_id: n };
    // 离开两着：第 1 着的请求已经发出，继续生成并写给第 1 着；第 2 着还在排队，不再发送
    service.coach_leave(node(1)).unwrap();
    service.coach_leave(node(2)).unwrap();
    rec.coach("第 1 着完成", 1, "ready");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(llm.count(), 1, "排队的第 2 着离开后不再发送");
    let second = service.coach_view(node(2)).unwrap();
    assert_eq!(second.status, CoachStatus::None, "{:?}", second.reason);
    let first = service.coach_view(node(1)).unwrap();
    assert!(first.result_current, "已经发出的请求返回后归属第 1 着");
    // 回到第 2 着时重新排队并生成
    rec.clear();
    explain(&service, &sid, 2, false);
    rec.coach("第 2 着完成", 2, "ready");
    assert_eq!(llm.count(), 2);
    service.shutdown();
}

#[test]
fn a_cancelled_request_that_returns_is_cached_for_the_next_view() {
    let dir = tempfile::tempdir().unwrap();
    let (service, llm, rec, sid) = two_moves_with_slow_first_call(dir.path());
    let node = |n| NodeRef { study_id: sid.clone(), node_id: n };
    // 取消已经发出的第 1 着：讲解状态回到空闲；请求返回后不改动状态，但结果存入缓存
    service.coach_cancel(node(1)).unwrap();
    rec.coach("第 2 着得到名额并完成", 2, "ready");
    let first = service.coach_view(node(1)).unwrap();
    assert!(first.record.as_ref().is_none_or(|r| r.result.is_none()), "取消后不把结果写入讲解状态");
    let calls = llm.count();
    rec.clear();
    explain(&service, &sid, 1, false);
    rec.coach("第 1 着使用缓存", 1, "ready");
    assert_eq!(llm.count(), calls, "再次请求第 1 着时直接使用缓存，不再调用模型");
    service.shutdown();
}

#[test]
fn asking_about_a_comparison_answers_from_its_evidence_and_keeps_the_answer() {
    // 第 2 次请求（“对方的回应”）的输出不合格，修正后合格
    let llm = mock_llm(|i| (Reply::Content(if i == 1 { bad_answer } else { valid_answer }), 0));
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), &format!("@go\n{ROUND}"), &llm, rec.clone());
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk-secret".into()) }).unwrap();
    let sid = standard_study(&service);
    let q = |kind| Question { study_id: sid.clone(), node_id: ROOT, mine: mv("h2e2"), reference: mv("h0g2"), kind };

    // 对比搜索还没有完成时不能提问，也不调用模型
    let early = service.ask_coach(q(QuestionKind::MyMove)).unwrap_err();
    assert!(early.message.contains("还没有完成"), "{}", early.message);
    let first = service.compare_moves(CompareRequest { study_id: sid.clone(), node_id: ROOT, mine: mv("h2e2"), reference: Some(mv("h0g2")) }).unwrap();
    if first.pending {
        rec.wait("对比完成", |e, v| e == "comparison_updated" && v["pending"] == false);
    }

    let asked = service.ask_coach(q(QuestionKind::MyMove)).unwrap();
    assert_eq!(asked.status, CoachStatus::Queued);
    let ready = rec.wait("回答", |e, v| e == "coach_answer_updated" && v["question"]["kind"] == "my_move" && v["status"] == "ready");
    let result = &ready["record"]["result"];
    assert_eq!(result["output"]["check"], "中卒现在由哪些棋子保护？");
    assert!(result["claims"].as_array().unwrap().iter().all(|c| c["group"] == "mine"), "保存引用到的结论原文：{result}");
    assert_eq!(result["demos"][0]["moves"], json!(["h2e2", "h9g7"]), "演示由程序按问题选择：{result}");
    // 请求只包含这组对比的证据；密钥只在请求头中
    let (auth, body) = llm.call(0);
    assert_eq!(auth.as_deref(), Some("Bearer sk-secret"));
    assert!(!body.to_string().contains("sk-secret"));
    let p = pack(&body);
    assert_eq!(p["question"]["text"], "我的选择炮二平五起了什么作用？");
    assert_eq!(p["mine"]["line"], json!(["炮二平五", "马8进7"]));
    assert!(p["claims"].as_array().unwrap().iter().any(|c| c["group"] == "difference"));

    // 同一个问题、同样的依据与设置：直接返回，不再调用模型
    let again = service.ask_coach(q(QuestionKind::MyMove)).unwrap();
    assert_eq!((again.status, again.result_current), (CoachStatus::Ready, true));
    assert_eq!(llm.count(), 1);

    // 输出不合格时按检查结果修正一次；每次调用都计入每日次数
    service.ask_coach(q(QuestionKind::Reply)).unwrap();
    rec.wait("修正后的回答", |e, v| e == "coach_answer_updated" && v["question"]["kind"] == "reply" && v["status"] == "ready");
    assert_eq!(llm.count(), 3);
    let repair = llm.call(2).1;
    assert!(repair["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().contains("没有通过检查"));
    assert_eq!(service.status().coach.used_today, 3);

    // 回答归属于这组对比：再次打开时列出；换一个对照时没有
    let pair = |reference: &str| AnswersRequest { study_id: sid.clone(), node_id: ROOT, mine: mv("h2e2"), reference: mv(reference) };
    let listed = service.coach_answers(pair("h0g2")).unwrap();
    assert_eq!(listed.iter().map(|a| (a.question.kind, a.status)).collect::<Vec<_>>(), [
        (QuestionKind::MyMove, CoachStatus::Ready),
        (QuestionKind::Reply, CoachStatus::Ready)
    ]);
    assert!(service.coach_answers(pair("b0c2")).unwrap().is_empty());

    // 讲解设置改变后标为过期，可以重新回答
    let mut cfg = service.status().config;
    cfg.coach.audience = Audience::Advanced;
    service.update_config(cfg).unwrap();
    let stale = service.coach_answers(pair("h0g2")).unwrap();
    assert!(stale.iter().all(|a| a.status == CoachStatus::Stale && !a.result_current), "{:?}", stale.iter().map(|a| &a.reason).collect::<Vec<_>>());
    // 状态版本递增：设置改变前读取的“当前有效”不能在界面上覆盖之后的“已过期”
    assert!(stale.iter().all(|a| a.seq > listed[0].seq), "设置改变后状态版本递增");
    assert!(stale[0].reason.as_deref().unwrap().contains("设置已改变"));
    assert!(stale[0].record.as_ref().unwrap().result.is_some(), "过期的回答仍然保留");
    service.shutdown();
}
