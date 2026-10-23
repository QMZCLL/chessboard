//! 引擎服务测试。
//!
//! - 模拟引擎（xq-mock-uci）按脚本输出，稳定复现排名交换、部分更新、提前结束、崩溃与设置变更。
//! - 真实引擎使用仓库自带的 Pikafish；缺少引擎文件时测试失败，而不是静默跳过。
//!   确实无法运行时（如 CPU 不支持 BMI2）可设置 `XQ_SKIP_ENGINE_TESTS=1` 明确跳过。

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use xq_app::engine::EngineExpectation;
use xq_app::engine::EngineService;
use xq_app::engine::EngineSpec;
use xq_app::engine::EngineState;
use xq_app::engine::JobSpec;
use xq_app::engine::Priority;
use xq_app::engine::SearchOutcome;
use xq_app::engine::SearchRequest;
use xq_app::engine::SearchResult;
use xq_core::Move;
use xq_core::fen::START_FEN;
use xq_core::rules::GameStatus;
use xq_core::study::RuleSet;

fn libs() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../libs/pikafish") }

fn bundled_engine() -> PathBuf {
    let name = if cfg!(windows) {
        "pikafish-windows.exe"
    } else if cfg!(target_os = "macos") {
        "pikafish-macos"
    } else {
        "pikafish-linux"
    };
    libs().join(name)
}

fn wait_ready(service: &EngineService) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !matches!(service.state(), EngineState::Ready { .. } | EngineState::Failed { .. }) {
        assert!(Instant::now() < deadline, "引擎启动超时");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start(binary: PathBuf, args: Vec<String>) -> EngineService {
    let spec = EngineSpec {
        binary,
        args,
        nnue: Some(libs().join("pikafish.nnue")),
        threads: 1,
        hash_mb: 16,
        rules: RuleSet::default(),
    };
    let service = EngineService::start(spec, Box::new(|_| {})).unwrap();
    wait_ready(&service);
    service
}

/// 真实引擎；缺少引擎文件时直接失败，除非明确要求跳过。
fn real_engine() -> Option<EngineService> {
    if std::env::var_os("XQ_SKIP_ENGINE_TESTS").is_some() {
        eprintln!("已按 XQ_SKIP_ENGINE_TESTS 跳过真实引擎测试");
        return None;
    }
    let bin = bundled_engine();
    assert!(bin.exists(), "缺少引擎 {}；如需跳过请设置 XQ_SKIP_ENGINE_TESTS=1", bin.display());
    let service = start(bin, Vec::new());
    assert!(matches!(service.state(), EngineState::Ready { .. }), "真实引擎没有就绪：{:?}", service.state());
    Some(service)
}

/// 模拟引擎：脚本写入临时目录，返回的目录需要与服务同时存活。
fn mock_engine(script: &str) -> (EngineService, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.txt");
    std::fs::write(&path, script).unwrap();
    let service = start(PathBuf::from(env!("CARGO_BIN_EXE_xq-mock-uci")), vec![path.display().to_string()]);
    assert!(matches!(service.state(), EngineState::Ready { .. }), "{:?}", service.state());
    (service, dir)
}

fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

fn request(fen: &str, multipv: u32, depth: u32) -> SearchRequest {
    SearchRequest {
        base_fen: fen.into(),
        moves: vec![],
        multipv,
        depth: Some(depth),
        movetime_ms: Some(5000),
        searchmoves: vec![],
        expect: None,
    }
}

fn submit(service: &EngineService, priority: Priority, req: SearchRequest) -> (u64, mpsc::Receiver<SearchOutcome>) {
    let (tx, rx) = mpsc::channel();
    let id = service.submit(JobSpec {
        priority,
        group: "test".into(),
        request: req,
        on_progress: None,
        on_done: Box::new(move |o| {
            let _ = tx.send(o);
        }),
    });
    (id, rx)
}

fn wait(rx: &mpsc::Receiver<SearchOutcome>) -> SearchOutcome { rx.recv_timeout(Duration::from_secs(60)).expect("任务没有结束") }

fn completed(outcome: SearchOutcome) -> SearchResult {
    match outcome {
        SearchOutcome::Completed(r) => r,
        other => panic!("应完成搜索：{other:?}"),
    }
}

fn firsts(r: &SearchResult) -> Vec<String> { r.lines.iter().map(|l| l.mv.iccs()).collect() }

const A: &str = "h2e2";
const B: &str = "h0g2";

fn info(depth: u32, rank: u32, mv: &str, cp: i32) -> String {
    format!("info depth {depth} seldepth {depth} multipv {rank} score cp {cp} nodes 10 time 1 pv {mv}")
}

#[test]
fn mock_rank_swap_uses_latest_complete_round() {
    let script = [
        "@go".to_string(),
        info(1, 1, A, 30),
        info(1, 2, B, 20),
        info(2, 1, B, 40),
        info(2, 2, A, 35),
        format!("bestmove {B}"),
    ]
    .join("\n");
    let (service, _dir) = mock_engine(&script);
    let r = completed(wait(&submit(&service, Priority::Interactive, request(START_FEN, 2, 10)).1));
    assert_eq!(firsts(&r), vec![B, A]);
    assert!(r.complete, "{:?}", r.notes);
    assert_eq!(r.depth, 2);
}

#[test]
fn mock_partial_round_is_not_mixed_in() {
    // 深度 2 只更新了第一名：旧做法会得到 [B, B]（不同深度混合、首着重复）
    let script = [
        "@go".to_string(),
        info(1, 1, A, 30),
        info(1, 2, B, 20),
        info(2, 1, B, 40),
        format!("bestmove {B}"),
    ]
    .join("\n");
    let (service, _dir) = mock_engine(&script);
    let r = completed(wait(&submit(&service, Priority::Interactive, request(START_FEN, 2, 10)).1));
    assert_eq!(firsts(&r), vec![A, B], "使用深度 1 的完整一轮");
    assert!(!r.complete, "最终着法与候选首位不一致时不能视为完整");
    assert!(r.notes.iter().any(|n| n.contains("深度 2")), "{:?}", r.notes);
    assert!(r.notes.iter().any(|n| n.contains("不一致")), "{:?}", r.notes);
}

#[test]
fn mock_incomplete_and_empty_searches() {
    // 从未凑齐一轮：只能给出部分结果并说明
    let script = ["@go".to_string(), info(1, 1, A, 30), format!("bestmove {A}"), "@go".into(), format!("bestmove {A}")]
        .join("\n");
    let (service, _dir) = mock_engine(&script);
    let r = completed(wait(&submit(&service, Priority::Interactive, request(START_FEN, 2, 10)).1));
    assert!(!r.complete);
    assert!(r.notes.iter().any(|n| n.contains("没有完成一轮完整")), "{:?}", r.notes);
    // 只有 bestmove、没有搜索信息
    assert!(matches!(wait(&submit(&service, Priority::Interactive, request(START_FEN, 2, 10)).1), SearchOutcome::Failed(_)));
}

#[test]
fn mock_crash_is_detected_and_engine_restarts() {
    let (service, _dir) = mock_engine("@go\nexit 3\n");
    match wait(&submit(&service, Priority::Interactive, request(START_FEN, 1, 4)).1) {
        SearchOutcome::Failed(reason) => assert!(reason.contains("意外退出"), "{reason}"),
        other => panic!("{other:?}"),
    }
    // 自动重启后恢复就绪（重启后脚本从头开始）
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(service.state(), EngineState::Ready { .. }) {
        assert!(Instant::now() < deadline, "{:?}", service.state());
        std::thread::sleep(Duration::from_millis(20));
    }
    // 连续多次崩溃后停止自动重启，并说明原因
    for _ in 0..3 {
        let _ = wait(&submit(&service, Priority::Interactive, request(START_FEN, 1, 4)).1);
        std::thread::sleep(Duration::from_millis(50));
    }
    match service.state() {
        EngineState::Failed { reason } => assert!(reason.contains("手动重启"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        wait(&submit(&service, Priority::Interactive, request(START_FEN, 1, 4)).1),
        SearchOutcome::Failed(r) if r.contains("引擎不可用")
    ));
}

#[test]
fn mock_stale_settings_are_rejected() {
    let script = ["@go".to_string(), info(1, 1, A, 30), format!("bestmove {A}")].join("\n");
    let (service, _dir) = mock_engine(&script);
    let EngineState::Ready { identity, .. } = service.state() else { panic!() };
    let current = EngineExpectation {
        engine: identity.name.clone(),
        nnue: identity.nnue.clone(),
        score_type: identity.score_type.clone(),
        rules: RuleSet::default(),
    };
    // 按旧规则提交的任务：执行时设置已不同，不运行也不产生结果
    let old = EngineExpectation { rules: RuleSet { repetition_rule: "AsianRule".into(), sixty_move_rule: true }, ..current.clone() };
    let req = SearchRequest { expect: Some(old), ..request(START_FEN, 1, 4) };
    assert!(matches!(wait(&submit(&service, Priority::Interactive, req).1), SearchOutcome::Stale(_)));
    let req = SearchRequest { expect: Some(current), ..request(START_FEN, 1, 4) };
    assert!(matches!(wait(&submit(&service, Priority::Interactive, req).1), SearchOutcome::Completed(_)));
}

#[test]
fn handshake_multipv_and_searchmoves() {
    let Some(service) = real_engine() else { return };
    match service.state() {
        EngineState::Ready { identity, applied, .. } => {
            assert!(identity.name.starts_with("Pikafish"), "{identity:?}");
            assert_eq!(identity.score_type, "PawnValueNormalized");
            assert!(applied.iter().any(|o| o.name == "Repetition Rule" && o.value == "ChineseRule"));
            assert!(!applied.iter().any(|o| o.name == "UCI_ShowWDL"), "不设置引擎不支持的选项");
        }
        other => panic!("{other:?}"),
    }
    let r = completed(wait(&submit(&service, Priority::Interactive, request(START_FEN, 3, 8)).1));
    assert_eq!(r.lines.len(), 3);
    let mut moves = firsts(&r);
    moves.sort();
    moves.dedup();
    assert_eq!(moves.len(), 3, "同一轮候选首着互不重复");
    assert!(r.complete, "{:?}", r.notes);
    assert_eq!(r.lines[0].pv[0], r.lines[0].mv);
    assert!(r.lines.iter().all(|l| l.depth == r.depth), "候选来自同一深度");

    let mut req = request(START_FEN, 3, 8);
    req.searchmoves = vec![mv("h0i2")];
    let r = completed(wait(&submit(&service, Priority::Interactive, req).1));
    assert_eq!(r.lines.len(), 1);
    assert_eq!(r.lines[0].mv, mv("h0i2"));

    // 带历史着法的局面
    let req = SearchRequest { moves: vec![mv("h2e2"), mv("h9g7")], ..request(START_FEN, 1, 6) };
    assert!(matches!(wait(&submit(&service, Priority::Interactive, req).1), SearchOutcome::Completed(_)));
}

#[test]
fn terminal_positions_skip_engine() {
    let Some(service) = real_engine() else { return };
    let (_, rx) = submit(&service, Priority::Interactive, request("3k5/3R5/3R5/9/9/9/9/9/9/4K4 b", 1, 8));
    assert!(matches!(wait(&rx), SearchOutcome::Terminal(GameStatus::Checkmate)));
    let (_, rx) = submit(&service, Priority::Interactive, request("3k5/4R4/4R4/9/9/9/9/9/9/4K4 b", 1, 8));
    assert!(matches!(wait(&rx), SearchOutcome::Terminal(GameStatus::Stalemate)));
    let bad = SearchRequest { moves: vec![mv("h0f1")], ..request(START_FEN, 1, 4) };
    assert!(matches!(wait(&submit(&service, Priority::Interactive, bad).1), SearchOutcome::Failed(_)));
}

#[test]
fn cancel_and_preemption() {
    let Some(service) = real_engine() else { return };
    let long = SearchRequest { depth: Some(99), movetime_ms: Some(20_000), ..request(START_FEN, 1, 99) };
    let (id, rx) = submit(&service, Priority::Interactive, long.clone());
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    service.cancel(id);
    assert!(matches!(wait(&rx), SearchOutcome::Cancelled));
    assert!(t.elapsed() < Duration::from_secs(3), "取消应在 stop 后很快结束");

    // 后台任务被交互任务抢占，交互任务先完成，后台任务之后重新完成
    let background = SearchRequest { movetime_ms: Some(1500), ..long };
    let (_, bg) = submit(&service, Priority::Background, background);
    std::thread::sleep(Duration::from_millis(200));
    let (_, fg) = submit(&service, Priority::Interactive, request(START_FEN, 1, 6));
    assert!(matches!(wait(&fg), SearchOutcome::Completed(_)));
    assert!(bg.try_recv().is_err(), "后台任务应被抢占后重新排队");
    assert!(matches!(wait(&bg), SearchOutcome::Completed(_)));
}

#[test]
fn missing_binary_reports_reason() {
    let service = start(PathBuf::from("/nonexistent/pikafish"), Vec::new());
    match service.state() {
        EngineState::Failed { reason } => assert!(reason.contains("无法启动引擎"), "{reason}"),
        other => panic!("{other:?}"),
    }
}
