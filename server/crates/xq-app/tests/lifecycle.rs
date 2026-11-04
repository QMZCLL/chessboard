//! 后台任务的生命周期（见 `jobs`）：每个任务最终都进入结束状态；删除、取消与取代之后到达的结果不写入，也不改变任务记录；
//! 退出时不留下进行中的任务与引擎进程，下次启动时记下上次中断的任务。
//!
//! 使用可编排的模拟引擎（`wait-stop` 让一次搜索停住，直到收到 stop）与本机的模拟模型服务。

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::api::*;
use xq_app::archive::ArchiveSource;
use xq_app::capture::SimBackend;
use xq_app::config::CoachProvider;
use xq_app::engine::EngineState;
use xq_app::events::EventSink;
use xq_app::jobs::JobKind;
use xq_app::jobs::JobRecord;
use xq_app::jobs::JobState;
use xq_app::records::Budget;
use xq_core::Move;
use xq_core::study::ROOT;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: &str, _payload: Value) {}
}

fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

const ROUND: &str = "info depth 1 seldepth 1 multipv 1 score cp 30 nodes 10 time 1 pv h2e2 h9g7\n\
    info depth 1 seldepth 1 multipv 2 score cp 20 nodes 10 time 1 pv h0g2\n\
    bestmove h2e2\n";

/// 模拟模型服务：回复前等待 `delay` 毫秒（可以随时改变），回复合格的讲解。
struct MockLlm {
    url: String,
    delay: Arc<AtomicU64>,
    calls: Arc<AtomicU64>,
}

fn mock_llm() -> MockLlm {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    let delay = Arc::new(AtomicU64::new(0));
    let calls = Arc::new(AtomicU64::new(0));
    let (d, c) = (delay.clone(), calls.clone());
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            c.fetch_add(1, Ordering::SeqCst);
            let wait = d.load(Ordering::SeqCst);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(wait));
                let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let text = json!({ "choices": [{ "message": { "role": "assistant", "content": valid_output(&body) }, "finish_reason": "stop" }] });
                let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let _ = request.respond(tiny_http::Response::from_string(text.to_string()).with_header(header));
            });
        }
    });
    MockLlm { url, delay, calls }
}

fn valid_output(body: &Value) -> String {
    let user = body["messages"][1]["content"].as_str().unwrap_or("{}");
    let pack = serde_json::from_str::<Value>(user).unwrap_or(Value::Null)["evidence"].clone();
    let claim = |groups: &[&str]| -> String {
        pack["claims"]
            .as_array()
            .and_then(|c| c.iter().find(|c| groups.contains(&c["group"].as_str().unwrap_or(""))))
            .and_then(|c| c["id"].as_str())
            .unwrap_or("c1")
            .to_string()
    };
    json!({
        "summary": { "text": "这步让炮来到中路，加强对中路的控制。", "claim_ids": [claim(&["basis"])] },
        "sections": [
            { "kind": "purpose", "text": "当前没有直接威胁，可以着手出动子力。", "claim_ids": [claim(&["need"])] },
            { "kind": "comparison", "text": "候选比较的结论见下方程序给出的评价。", "claim_ids": [claim(&["candidate", "comparison"])] },
        ],
        "lesson": { "text": "出子时同时检查新增的攻击和保护关系。", "claim_ids": [claim(&["lesson"])] },
        "followups": [],
    })
    .to_string()
}

fn repo_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..") }

fn engine_binary(dir: &Path) -> PathBuf {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_xq-mock-uci"));
    dir.join("engine").join(exe.file_name().unwrap())
}

/// 启动服务：模拟引擎按 `script` 回答，AI 讲解使用模拟模型服务。`script` 为空时沿用目录中已有的脚本。
fn start(dir: &Path, script: Option<&str>, llm: &MockLlm) -> AppService {
    let engine_dir = dir.join("engine");
    std::fs::create_dir_all(&engine_dir).unwrap();
    let binary = engine_binary(dir);
    if !binary.exists() {
        std::fs::copy(env!("CARGO_BIN_EXE_xq-mock-uci"), &binary).unwrap();
    }
    if let Some(script) = script {
        std::fs::write(engine_dir.join("mock-uci-script.txt"), script).unwrap();
    }
    let mut cfg = xq_app::config::AppConfig::default();
    cfg.engine.binary = Some(binary);
    cfg.analysis.multipv = 2;
    cfg.analysis.auto_explain = true;
    cfg.coach.enabled = true;
    cfg.coach.provider = CoachProvider::Deepseek;
    cfg.coach.base_url = llm.url.clone();
    cfg.coach.model = "mock-model".into();
    cfg.coach.timeout_ms = 20_000;
    xq_app::config::save(&dir.join("config").join("config.json"), &cfg).unwrap();
    let paths = AppPaths {
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        log_dir: dir.join("logs"),
        resource_dir: repo_root(),
        legacy_config: None,
    };
    let service = AppService::start(paths, Arc::new(Quiet), Arc::new(SimBackend::new())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !matches!(service.status().engine, EngineState::Ready { .. }) {
        assert!(Instant::now() < deadline, "{:?}", service.status().engine);
        std::thread::sleep(Duration::from_millis(20));
    }
    service
}

fn until(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(Instant::now() < deadline, "等待超时：{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn jobs_of(service: &AppService, kind: JobKind) -> Vec<JobRecord> {
    let snap = service.jobs();
    snap.active.into_iter().chain(snap.finished).filter(|r| r.kind == kind).collect()
}

fn standard_study(service: &AppService) -> String {
    let req = CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None };
    service.create_study(req).unwrap().study.study.id
}

/// 引擎中仍在运行的模拟引擎进程（按程序路径找，只在 Linux 上检查）。
#[cfg(target_os = "linux")]
fn engine_processes(binary: &Path) -> usize {
    let target = binary.canonicalize().unwrap();
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()))
        .filter(|e| std::fs::read_link(e.path().join("exe")).is_ok_and(|p| p == target))
        // 已经退出、等待回收的进程不算
        .filter(|e| std::fs::read_to_string(e.path().join("stat")).is_ok_and(|s| !s.contains(") Z ")))
        .count()
}

#[test]
fn results_arriving_after_delete_or_cancel_change_nothing_and_every_job_ends() {
    let llm = mock_llm();
    let dir = tempfile::tempdir().unwrap();
    // 第一次搜索停住，直到删除这一着时引擎收到 stop；之后的搜索立即完成
    let script = format!("@go\nwait-stop\n{ROUND}@go\n{ROUND}@go\nwait-stop\n{ROUND}");
    let service = start(dir.path(), Some(&script), &llm);
    let sid = standard_study(&service);

    // 讲解进行中删除这一着：任务记为取消，引擎随后返回的结果不写入，也不改变任务记录
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap();
    let node = first.node_id.unwrap();
    until("讲解开始", || jobs_of(&service, JobKind::Explain).iter().any(|r| r.state == JobState::Running));
    let late_before = service.jobs().late;
    service.delete_node(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    until("停住的搜索返回", || service.jobs().late > late_before);
    let explain = jobs_of(&service, JobKind::Explain);
    assert_eq!(explain.len(), 1, "{explain:?}");
    assert_eq!((explain[0].state, explain[0].committed), (JobState::Cancelled, false));
    assert_eq!(explain[0].detail.as_deref(), Some("这一着已删除"));
    assert_eq!(explain[0].study_id.as_deref(), Some(sid.as_str()));
    assert!(explain[0].subject_revision.is_some(), "记下任务开始时的棋谱版本");
    assert!(service.node_detail(NodeRef { study_id: sid.clone(), node_id: node }).is_err());
    assert!(service.study_view(&sid).unwrap().explain_status.is_empty(), "删除之后没有留下这一着的讲解");

    // 再走一次：新的节点，讲解成功并写入
    let again = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap();
    let node = again.node_id.unwrap();
    until("讲解完成", || service.study_view(&sid).unwrap().stats.pending == 0);
    assert!(jobs_of(&service, JobKind::Explain).iter().any(|r| r.state == JobState::Succeeded && r.committed));

    // 局面分析：取消后记为取消
    service
        .analyze_position(AnalyzeRequest { study_id: sid.clone(), node_id: node, budget: Budget::Quick, request_id: "r1".into() })
        .unwrap();
    until("分析开始", || jobs_of(&service, JobKind::Analyze).iter().any(|r| r.state == JobState::Running));
    service.cancel_analysis("r1");
    until("分析结束", || jobs_of(&service, JobKind::Analyze).iter().all(|r| r.state.is_final()));
    let analyze = jobs_of(&service, JobKind::Analyze);
    assert_eq!((analyze[0].state, analyze[0].operation_id.as_str()), (JobState::Cancelled, "r1"));

    // AI 讲解：生成中取消，模型随后返回的结果不改变讲解状态与任务记录；再次请求时生成并写入
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk-test".into()) }).unwrap();
    llm.delay.store(800, Ordering::SeqCst);
    let req = || CoachExplainRequest { study_id: sid.clone(), node_id: node, audience: None, detail: None, refresh: false };
    service.coach_explain(req()).unwrap();
    until("AI 讲解开始", || jobs_of(&service, JobKind::Coach).iter().any(|r| r.state == JobState::Running));
    service.coach_cancel(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    let late_before = service.jobs().late;
    until("取消后模型返回", || service.jobs().late > late_before);
    let coach = jobs_of(&service, JobKind::Coach);
    assert_eq!((coach[0].state, coach[0].detail.as_deref()), (JobState::Cancelled, Some("已取消")), "{coach:?}");
    let view = service.coach_view(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    assert!(view.record.as_ref().is_none_or(|r| r.result.is_none()), "取消后返回的结果没有写给这一着");
    llm.delay.store(0, Ordering::SeqCst);
    service.coach_explain(CoachExplainRequest { refresh: true, ..req() }).unwrap();
    until("AI 讲解完成", || jobs_of(&service, JobKind::Coach).iter().any(|r| r.state == JobState::Succeeded && r.committed));

    // 同步完成的操作也登记：出题、导出与导入
    service.generate_exercises(xq_app::exercise::ExerciseRequest { study_id: sid.clone(), regenerate: false }).unwrap();
    let file = dir.path().join("档案 备份.json");
    service.export_archive_to_file(&file).unwrap();
    service.import_archive(ArchiveSource::File { path: file }).unwrap();
    let broken = service.import_archive(ArchiveSource::Text { text: "{".into() });
    assert!(broken.is_err());

    assert!(service.wait_jobs_idle(Duration::from_secs(10)), "全部任务都结束：{:?}", service.jobs().active);
    let snap = service.jobs();
    assert!(snap.finished.iter().all(|r| r.state.is_final() && r.finished_at.is_some() && !r.operation_id.is_empty()));
    let kinds: Vec<(JobKind, JobState)> = snap.finished.iter().map(|r| (r.kind, r.state)).collect();
    for expected in [
        (JobKind::Exercises, JobState::Succeeded),
        (JobKind::Export, JobState::Succeeded),
        (JobKind::ImportArchive, JobState::Succeeded),
        (JobKind::ImportArchive, JobState::Failed),
    ] {
        assert!(kinds.contains(&expected), "缺少 {expected:?}：{kinds:?}");
    }
    assert!(snap.finished.iter().all(|r| r.committed == (r.state == JobState::Succeeded && r.committed)), "只有成功的任务写入");
    service.shutdown();
}

#[test]
fn exit_ends_every_job_leaves_no_engine_process_and_the_next_start_records_what_was_interrupted() {
    let llm = mock_llm();
    let dir = tempfile::tempdir().unwrap();
    // 第一着的讲解立即完成；之后的搜索都停住
    let script = format!("@go\n{ROUND}@go\nwait-stop\n{ROUND}@go\nwait-stop\n{ROUND}");
    let service = start(dir.path(), Some(&script), &llm);
    let sid = standard_study(&service);
    let node = service
        .apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false })
        .unwrap()
        .node_id
        .unwrap();
    until("第一着讲解完成", || service.study_view(&sid).unwrap().stats.pending == 0);
    // AI 讲解生成中（模型服务 4 秒后才回答，超过退出时等待的 3 秒），第二着的讲解与局面分析停在引擎中
    service.set_coach_credential(CoachCredentialRequest { key: Some("sk-test".into()) }).unwrap();
    llm.delay.store(4_000, Ordering::SeqCst);
    service.coach_explain(CoachExplainRequest { study_id: sid.clone(), node_id: node, audience: None, detail: None, refresh: false }).unwrap();
    until("AI 讲解开始", || llm.calls.load(Ordering::SeqCst) > 0);
    service.apply_move(MoveRequest { study_id: sid.clone(), parent: node, mv: mv("h9g7"), variation: false }).unwrap();
    service
        .analyze_position(AnalyzeRequest { study_id: sid.clone(), node_id: node, budget: Budget::Quick, request_id: "held".into() })
        .unwrap();
    until("任务都在进行", || service.jobs().active.len() >= 3);

    let started = Instant::now();
    service.shutdown();
    assert!(started.elapsed() < Duration::from_secs(10), "退出不会一直等待：{:?}", started.elapsed());
    let snap = service.jobs();
    assert!(snap.active.is_empty(), "退出后没有进行中的任务：{:?}", snap.active);
    let coach = snap.finished.iter().find(|r| r.kind == JobKind::Coach).unwrap();
    assert_eq!(coach.state, JobState::Interrupted, "{coach:?}");
    assert!(
        snap.finished.iter().filter(|r| r.kind == JobKind::Explain).any(|r| matches!(r.state, JobState::Cancelled | JobState::Interrupted)),
        "{:?}",
        snap.finished
    );
    #[cfg(target_os = "linux")]
    assert_eq!(engine_processes(&engine_binary(dir.path())), 0, "退出后没有留下引擎进程");
    drop(service);

    // 下次启动：上次没有完成的 AI 讲解标为中断并记入任务登记，界面不会一直显示生成中。
    // 已经发出的模型请求不能中途取消：退出时最多等 3 秒，之后进程结束。这里在同一个进程中重新启动，
    // 要等那个请求返回、线程放开数据库
    llm.delay.store(0, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(15);
    while xq_app::storage::Store::open(&dir.path().join("data/xqlink.redb")).is_err() {
        assert!(Instant::now() < deadline, "退出后数据库一直被占用");
        std::thread::sleep(Duration::from_millis(50));
    }
    let service = start(dir.path(), Some(&format!("@go\n{ROUND}")), &llm);
    let interrupted: Vec<JobRecord> =
        service.jobs().finished.into_iter().filter(|r| r.state == JobState::Interrupted).collect();
    assert!(interrupted.iter().any(|r| r.kind == JobKind::Coach), "{interrupted:?}");
    let view = service.coach_view(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    assert!(!matches!(view.status, xq_app::llm::CoachStatus::Queued | xq_app::llm::CoachStatus::Running), "{:?}", view.status);
    service.shutdown();
}
