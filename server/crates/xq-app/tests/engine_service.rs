//! 用仓库自带的 Pikafish 验证引擎服务（仅在 Linux x86-64 上运行真实引擎）。

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use xq_app::engine::EngineService;
use xq_app::engine::EngineSpec;
use xq_app::engine::EngineState;
use xq_app::engine::JobSpec;
use xq_app::engine::Priority;
use xq_app::engine::SearchOutcome;
use xq_app::engine::SearchRequest;
use xq_core::Move;
use xq_core::fen::START_FEN;
use xq_core::rules::GameStatus;
use xq_core::study::RuleSet;

fn libs() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../libs/pikafish") }

fn spec(binary: PathBuf) -> EngineSpec {
    EngineSpec { binary, nnue: Some(libs().join("pikafish.nnue")), threads: 1, hash_mb: 16, rules: RuleSet::default() }
}

fn start(binary: PathBuf) -> EngineService {
    let service = EngineService::start(spec(binary), Box::new(|_| {})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !matches!(service.state(), EngineState::Ready { .. } | EngineState::Failed { .. }) {
        assert!(Instant::now() < deadline, "引擎启动超时");
        std::thread::sleep(Duration::from_millis(50));
    }
    service
}

fn real_engine() -> Option<EngineService> {
    let bin = libs().join("pikafish-linux");
    (cfg!(all(target_os = "linux", target_arch = "x86_64")) && bin.exists()).then(|| start(bin))
}

fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

fn request(fen: &str, multipv: u32, depth: u32) -> SearchRequest {
    SearchRequest { base_fen: fen.into(), moves: vec![], multipv, depth: Some(depth), movetime_ms: Some(5000), searchmoves: vec![] }
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
    let (_, rx) = submit(&service, Priority::Interactive, request(START_FEN, 3, 8));
    let SearchOutcome::Completed(result) = wait(&rx) else { panic!("应完成搜索") };
    assert_eq!(result.lines.len(), 3);
    let mut moves: Vec<Move> = result.lines.iter().map(|l| l.mv).collect();
    moves.dedup();
    assert_eq!(moves.len(), 3);
    assert!(result.depth >= 8);
    assert_eq!(result.lines[0].pv[0], result.lines[0].mv);

    let mut req = request(START_FEN, 3, 8);
    req.searchmoves = vec![mv("h0i2")];
    let (_, rx) = submit(&service, Priority::Interactive, req);
    let SearchOutcome::Completed(result) = wait(&rx) else { panic!("应完成搜索") };
    assert_eq!(result.lines.len(), 1);
    assert_eq!(result.lines[0].mv, mv("h0i2"));

    // 带历史着法的局面
    let req = SearchRequest { moves: vec![mv("h2e2"), mv("h9g7")], ..request(START_FEN, 1, 6) };
    let (_, rx) = submit(&service, Priority::Interactive, req);
    assert!(matches!(wait(&rx), SearchOutcome::Completed(_)));
}

#[test]
fn terminal_positions_skip_engine() {
    let Some(service) = real_engine() else { return };
    let (_, rx) = submit(&service, Priority::Interactive, request("3k5/3R5/3R5/9/9/9/9/9/9/4K4 b", 1, 8));
    assert!(matches!(wait(&rx), SearchOutcome::Terminal(GameStatus::Checkmate)));
    let (_, rx) = submit(&service, Priority::Interactive, request("3k5/4R4/4R4/9/9/9/9/9/9/4K4 b", 1, 8));
    assert!(matches!(wait(&rx), SearchOutcome::Terminal(GameStatus::Stalemate)));
    let bad = SearchRequest { moves: vec![mv("h0f1")], ..request(START_FEN, 1, 4) };
    let (_, rx) = submit(&service, Priority::Interactive, bad);
    assert!(matches!(wait(&rx), SearchOutcome::Failed(_)));
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

#[cfg(unix)]
#[test]
fn crash_is_detected_and_engine_restarts() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("fake-engine.sh");
    // 握手正常，收到 go 后直接退出，模拟引擎崩溃
    std::fs::write(
        &script,
        "#!/bin/sh\nwhile read line; do\n  case \"$line\" in\n    uci) echo 'id name FakeEngine'; echo 'option name Threads type spin default 1 min 1 max 8'; echo uciok;;\n    isready) echo readyok;;\n    go*) exit 3;;\n    quit) exit 0;;\n  esac\ndone\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let service = start(script);
    assert!(matches!(service.state(), EngineState::Ready { .. }), "{:?}", service.state());
    let (_, rx) = submit(&service, Priority::Interactive, request(START_FEN, 1, 4));
    match wait(&rx) {
        SearchOutcome::Failed(reason) => assert!(reason.contains("意外退出"), "{reason}"),
        other => panic!("{other:?}"),
    }
    // 自动重启后恢复就绪
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(service.state(), EngineState::Ready { .. }) {
        assert!(Instant::now() < deadline, "{:?}", service.state());
        std::thread::sleep(Duration::from_millis(50));
    }
    // 连续多次崩溃后停止自动重启，并说明原因
    for _ in 0..3 {
        let (_, rx) = submit(&service, Priority::Interactive, request(START_FEN, 1, 4));
        let _ = wait(&rx);
        std::thread::sleep(Duration::from_millis(100));
    }
    match service.state() {
        EngineState::Failed { reason } => assert!(reason.contains("手动重启"), "{reason}"),
        other => panic!("{other:?}"),
    }
    let (_, rx) = submit(&service, Priority::Interactive, request(START_FEN, 1, 4));
    assert!(matches!(wait(&rx), SearchOutcome::Failed(r) if r.contains("引擎不可用")));
}

#[test]
fn missing_binary_reports_reason() {
    let service = start(PathBuf::from("/nonexistent/pikafish"));
    match service.state() {
        EngineState::Failed { reason } => assert!(reason.contains("无法启动引擎"), "{reason}"),
        other => panic!("{other:?}"),
    }
}
