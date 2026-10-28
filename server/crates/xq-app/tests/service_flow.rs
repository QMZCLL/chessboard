//! 服务层端到端流程：真实引擎 + 模拟屏幕输入。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::api::*;
use xq_app::capture::CaptureState;
use xq_app::capture::SimBackend;
use xq_app::compare::CompareRequest;
use xq_app::engine::EngineState;
use xq_app::events::EventSink;
use xq_app::records::Budget;
use xq_app::archive::ArchiveSource;
use xq_app::decision::DecisionState;
use xq_app::decision::Intent;
use xq_app::decision::PracticeGoal;
use xq_app::review::ReviewAttemptRequest;
use xq_app::topics::Issue;
use xq_app::topics::Theme;
use xq_app::review::ReviewRef;
use xq_app::review::ReviewStatus;
use xq_app::review::ReviewUpdateRequest;
use xq_core::Board;
use xq_core::Color;
use xq_core::Move;
use xq_core::capture::Rect;
use xq_core::explain::hints::HintFocus;
use xq_core::explain::hints::HintMove;
use xq_core::explain::hints::UsedHints;
use xq_core::fen;
use xq_core::rules::GameStatus;
use xq_core::study::Orientation;
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
        self.wait_for(Duration::from_secs(60), pred).unwrap_or_else(|| panic!("等待事件超时：{what}"))
    }

    /// 在限定时间内等待事件，超时返回 None。
    fn wait_for(&self, timeout: Duration, pred: impl Fn(&str, &Value) -> bool) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        let mut events = self.events.lock().unwrap();
        loop {
            if let Some((_, v)) = events.iter().find(|(e, v)| pred(e, v)) {
                return Some(v.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            events = self.signal.wait_timeout(events, left).unwrap().0;
        }
    }

    fn clear(&self) { self.events.lock().unwrap().clear(); }
}

fn repo_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..") }

/// 这些流程依赖真实引擎；缺少引擎时服务启动会失败并报告原因，而不是静默通过。
/// 确实无法运行时可设置 `XQ_SKIP_ENGINE_TESTS=1` 明确跳过。
fn skip_engine_tests() -> bool {
    let skip = std::env::var_os("XQ_SKIP_ENGINE_TESTS").is_some();
    if skip {
        eprintln!("已按 XQ_SKIP_ENGINE_TESTS 跳过依赖真实引擎的流程测试");
    }
    skip
}

fn wait_engine(service: &AppService) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match service.status().engine {
            EngineState::Ready { .. } => return,
            EngineState::Failed { reason } => panic!("引擎不可用：{reason}"),
            other => assert!(Instant::now() < deadline, "{other:?}"),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn start(dir: &std::path::Path, recorder: Arc<Recorder>, sim: Arc<SimBackend>) -> AppService {
    let paths = AppPaths {
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        log_dir: dir.join("logs"),
        resource_dir: repo_root(),
        legacy_config: None,
    };
    let service = AppService::start(paths, recorder, sim).unwrap();
    wait_engine(&service);
    // 测试中缩短搜索预算
    let mut cfg = service.status().config;
    cfg.analysis.quick_time_ms = 300;
    cfg.analysis.quick_depth = 10;
    cfg.engine.threads = 1;
    cfg.engine.hash_mb = 16;
    service.update_config(cfg).unwrap();
    service
}

fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

#[test]
fn endgame_mate_and_terminal_analysis() {
    if skip_engine_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), rec.clone(), Arc::new(SimBackend::new()));

    // 同一残局分别设为红走、黑走
    let fen_board = "3k5/9/9/9/9/9/9/9/4R4/4K4";
    let missing_side = service.create_study(CreateStudyRequest {
        name: None,
        kind: CreateKind::Fen { fen: fen_board.into(), side: None },
        orientation: None,
        user_side: None,
    });
    assert!(missing_side.unwrap_err().message.contains("行棋方"));
    let black = service
        .create_study(CreateStudyRequest {
            name: None,
            kind: CreateKind::Setup { board: fen_board.into(), side: Color::Black },
            orientation: None,
            user_side: None,
        })
        .unwrap();
    assert_eq!(black.study.study.orientation, Orientation::BlackBottom, "黑走的残局默认黑方在下");
    let detail = service.node_detail(NodeRef { study_id: black.study.study.id.clone(), node_id: ROOT }).unwrap();
    assert!(detail.position.summary.starts_with("轮到黑方走"), "{}", detail.position.summary);
    assert!(!detail.history_complete);

    let red = service
        .create_study(CreateStudyRequest {
            name: Some("车杀".into()),
            kind: CreateKind::Fen { fen: format!("{fen_board} w"), side: None },
            orientation: None,
            user_side: None,
        })
        .unwrap();
    let sid = red.study.study.id.clone();

    // 非法走法有具体原因
    let resp = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("e1d2"), variation: false }).unwrap();
    assert!(resp.node_id.is_none());
    assert_eq!(resp.illegal.unwrap().code, "rook_line");

    // 车五平六将死：讲解完成，终局局面的分析不再崩溃
    service.focus_node(NodeRef { study_id: sid.clone(), node_id: 1 });
    let resp = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("e1d1"), variation: false }).unwrap();
    let node = resp.node_id.unwrap();
    let ev = rec.wait("详细讲解", |e, v| {
        e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed"
    });
    let summary = ev["record"]["explanation"]["summary"].as_str().unwrap();
    assert!(summary.contains("将死对方"), "{summary}");
    assert_eq!(ev["record"]["explanation"]["verdict"]["tone"], "best");
    assert_eq!(ev["stats"]["detailed"], 1);

    let detail = service.node_detail(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    assert_eq!(detail.status, GameStatus::Checkmate);
    assert!(detail.legal_moves.is_empty());
    service
        .analyze_position(AnalyzeRequest { study_id: sid.clone(), node_id: node, budget: Budget::Quick, request_id: "r-mate".into() })
        .unwrap();
    let done = rec.wait("终局分析", |e, v| e == "analysis_done" && v["request_id"] == "r-mate");
    assert!(done["error"].is_null(), "{done}");
    assert_eq!(done["position"]["status"], "checkmate");

    // 时间单位：界面 1.5 秒 → 1500 毫秒
    let mut cfg = service.status().config;
    cfg.analysis.deep_time_ms = 1500;
    let status = service.update_config(cfg).unwrap();
    assert_eq!(status.config.analysis.deep_time_ms, 1500);
    service
        .analyze_position(AnalyzeRequest { study_id: sid.clone(), node_id: ROOT, budget: Budget::Deep, request_id: "r-deep".into() })
        .unwrap();
    let done = rec.wait("深入分析", |e, v| e == "analysis_done" && v["request_id"] == "r-deep");
    let conditions = done["position"]["conditions"].as_str().unwrap();
    assert!(conditions.contains("≤1.5 秒"), "{conditions}");
    assert_eq!(done["position"]["candidates"][0]["mv"], "e1d1");
    service.shutdown();
}

#[test]
fn branches_persist_across_restart_and_export() {
    if skip_engine_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let sim = Arc::new(SimBackend::new());
    let service = start(dir.path(), rec.clone(), sim.clone());
    let imported = service
        .create_study(CreateStudyRequest {
            name: None,
            kind: CreateKind::Record { text: "1. 炮二平五 马8进7 2. 马二进三 车9平8 3. 车一进一".into() },
            orientation: None,
            user_side: None,
        })
        .unwrap();
    let sid = imported.study.study.id.clone();
    assert_eq!(imported.study.stats.moves, 5);
    // 每一着都有讲解任务：等待全部完成
    rec.wait("全部讲解", |e, v| e == "explanation_updated" && v["study_id"] == sid.as_str() && v["stats"]["detailed"] == 5);

    // 在第一步后试走分支
    let branch =
        service.apply_move(MoveRequest { study_id: sid.clone(), parent: 1, mv: mv("b9c7"), variation: true }).unwrap();
    let branch_node = branch.node_id.unwrap();
    service.set_note(NoteRequest { study_id: sid.clone(), node_id: branch_node, note: "另一种应法".into(), starred: true }).unwrap();
    rec.wait("分支讲解", |e, v| e == "explanation_updated" && v["node_id"] == branch_node && v["status"] == "detailed");
    let exported = service.export_study(&sid).unwrap();
    service.shutdown();
    drop(service);

    let service = start(dir.path(), rec.clone(), sim);
    let view = service.load_study(&sid).unwrap();
    assert_eq!(view.study.nodes.len(), 7);
    assert_eq!(view.study.node(1).unwrap().children.len(), 2, "主线与分支都保留");
    assert_eq!(view.study.node(branch_node).unwrap().note, "另一种应法");
    assert_eq!(view.stats.detailed, 6);
    let detail = service.node_detail(NodeRef { study_id: sid.clone(), node_id: 3 }).unwrap();
    let expl = detail.explanation.unwrap().explanation.unwrap();
    assert_eq!(expl.notation, "马二进三");
    assert!(!expl.candidates.is_empty());

    let copy = service.import_text(&exported, Some("副本".into())).unwrap();
    assert_ne!(copy.study.study.id, sid);
    assert_eq!(copy.study.study.nodes.len(), 7);
    assert_eq!(copy.study.stats.detailed, 6);
    service.shutdown();
}

fn wait_capture(rec: &Recorder, state: &str) -> Value {
    rec.wait(state, |e, v| e == "capture_state" && v["state"] == state)
}

#[test]
fn capture_flow_with_simulated_screen() {
    if skip_engine_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let sim = Arc::new(SimBackend::new());
    let service = start(dir.path(), rec.clone(), sim.clone());
    let mut cfg = service.status().config;
    cfg.capture.interval_ms = 50;
    cfg.capture.stable_frames = 2;
    cfg.capture.unexplained_frames = 2;
    cfg.analysis.auto_explain = false;
    service.update_config(cfg).unwrap();

    // 黑方在下的画面
    let mut screen = Board::startpos();
    sim.show(Some(screen.clone()), Orientation::BlackBottom);
    service.start_capture(StartCaptureRequest { window_id: 1, rect: Rect { x: 0.0, y: 0.0, w: 800.0, h: 900.0 } }).unwrap();
    let awaiting = wait_capture(&rec, "awaiting_start");
    assert_eq!(awaiting["observed"]["orientation"], "black_bottom");
    assert_eq!(awaiting["observed"]["suggested_side"], "red");
    let view = service
        .confirm_capture_start(ConfirmStartRequest {
            board: awaiting["observed"]["board"].as_str().unwrap().into(),
            side: Color::Red,
            orientation: Orientation::BlackBottom,
            name: None,
            attach: None,
        })
        .unwrap();
    let sid = view.study.id.clone();
    assert_eq!(view.study.user_side, Color::Black);
    rec.clear();

    // 红方走一步
    screen.apply(mv("h2e2"));
    sim.show(Some(screen.clone()), Orientation::BlackBottom);
    rec.wait("记录第一步", |e, v| e == "study_updated" && v["reason"] == "capture" && v["live_node"] == 1);
    // 漏掉中间一帧：画面直接出现两步之后
    screen.apply(mv("h9g7"));
    screen.apply(mv("h0g2"));
    sim.show(Some(screen.clone()), Orientation::BlackBottom);
    rec.wait("补全两步", |e, v| e == "study_updated" && v["live_node"] == 3);
    let study = service.load_study(&sid).unwrap().study;
    let notations: Vec<String> = study.mainline().iter().skip(1).map(|n| study.node(*n).unwrap().notation.clone().unwrap()).collect();
    assert_eq!(notations, vec!["炮二平五", "马8进7", "马二进三"]);

    // 无法解释的画面：等待确认，确认后建立缺口节点
    let odd = fen::parse_fen("3k5/9/9/9/9/9/9/9/4R4/4K4").unwrap().board;
    sim.show(Some(odd), Orientation::BlackBottom);
    let ask = wait_capture(&rec, "awaiting_confirmation");
    assert_eq!(ask["kind"], "unexplained");
    let state = service
        .resolve_capture(ResolveCaptureRequest::Accept { board: ask["observed"]["board"].as_str().unwrap().into(), side: Color::Black })
        .unwrap();
    assert!(matches!(state, CaptureState::Tracking { .. }), "{state:?}");
    let study = service.load_study(&sid).unwrap().study;
    let gap = study.node(study.live_node.unwrap()).unwrap();
    assert!(gap.gap && gap.mv.is_none());
    assert_eq!(gap.side_to_move, Color::Black);

    // 停止后不再记录
    assert_eq!(service.stop_capture(), CaptureState::Idle);
    assert!(service.load_study(&sid).unwrap().study.live_node.is_none());
    service.shutdown();
}

#[test]
fn record_import_reports_stop_point() {
    if skip_engine_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), rec.clone(), Arc::new(SimBackend::new()));
    let resp = service.import_text("1. 炮二平五 马8进7 2. 车一平二", None).unwrap();
    assert_eq!(resp.study.stats.moves, 2);
    assert_eq!(resp.stopped.unwrap().ply, 3);
    assert!(resp.notes[0].contains("第 3 步"));
    let validate = service.validate_position(ValidateRequest { fen: "4k4/9/9/9/9/9/9/9/9/4K4".into(), side: None });
    assert!(validate.needs_side);
    assert!(validate.report.unwrap().issues.iter().any(|i| i.code == "kings_facing"));
    service.shutdown();
}

#[test]
fn failed_line_insert_leaves_study_unchanged() {
    if skip_engine_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), rec.clone(), Arc::new(SimBackend::new()));
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let before = service.study_view(&sid).unwrap();
    // 第一步合法，第二步非法（红方连走两步）
    let result = service.add_line(LineRequest { study_id: sid.clone(), parent: ROOT, moves: vec![mv("h2e2"), mv("h0g2")] });
    assert!(result.is_err());
    let after = service.study_view(&sid).unwrap();
    assert_eq!(after.study.nodes.len(), before.study.nodes.len(), "内存中的棋谱不应留下第一步");
    assert_eq!(after.study.revision, before.study.revision);
    assert_eq!(after.stats.moves, 0);
    service.shutdown();
    drop(service);
    // 数据库中同样没有变化
    let service = start(dir.path(), rec, Arc::new(SimBackend::new()));
    assert_eq!(service.load_study(&sid).unwrap().study.nodes.len(), 1);
    service.shutdown();
}

#[test]
fn new_game_on_screen_is_not_written_to_old_study() {
    if skip_engine_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let sim = Arc::new(SimBackend::new());
    let service = start(dir.path(), rec.clone(), sim.clone());
    let mut cfg = service.status().config;
    cfg.capture.interval_ms = 40;
    cfg.capture.stable_frames = 2;
    cfg.capture.unexplained_frames = 2;
    cfg.analysis.auto_explain = false;
    service.update_config(cfg).unwrap();

    let mut screen = Board::startpos();
    sim.show(Some(screen.clone()), Orientation::RedBottom);
    service.start_capture(StartCaptureRequest { window_id: 1, rect: Rect { x: 0.0, y: 0.0, w: 800.0, h: 900.0 } }).unwrap();
    let awaiting = rec.wait("起始局面", |e, v| e == "capture_state" && v["state"] == "awaiting_start");
    let view = service
        .confirm_capture_start(ConfirmStartRequest {
            board: awaiting["observed"]["board"].as_str().unwrap().into(),
            side: Color::Red,
            orientation: Orientation::RedBottom,
            name: None,
            attach: None,
        })
        .unwrap();
    let sid = view.study.id.clone();
    for m in ["h2e2", "h9g7"] {
        screen.apply(mv(m));
        sim.show(Some(screen.clone()), Orientation::RedBottom);
        let n = service.study_view(&sid).unwrap().study.nodes.len();
        let deadline = Instant::now() + Duration::from_secs(10);
        while service.study_view(&sid).unwrap().study.nodes.len() == n {
            assert!(Instant::now() < deadline, "没有记录 {m}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    rec.clear();
    // 外部棋盘开始新的一局，并且在用户确认前已经走出第一步
    sim.show(Some(Board::startpos()), Orientation::RedBottom);
    let ask = rec.wait("新一局确认", |e, v| e == "capture_state" && v["state"] == "awaiting_confirmation");
    assert_eq!(ask["kind"], "new_game");
    let first = Board::startpos().applied(mv("b2e2"));
    sim.show(Some(first.clone()), Orientation::RedBottom);
    let updated = rec.wait("待确认画面更新", |e, v| {
        e == "capture_state" && v["state"] == "awaiting_confirmation" && v["observed"]["board"] == fen::board_fen(&first)
    });
    assert!(updated["observed"]["suggested_side"].is_null(), "已不在开局时不猜测行棋方");
    std::thread::sleep(Duration::from_millis(400));
    let old = service.study_view(&sid).unwrap().study;
    assert_eq!(old.move_nodes().count(), 2, "确认之前不能向旧棋谱写入新一局的着法");
    // 确认为新的一局后，切换到新会话继续记录
    let state = service
        .resolve_capture(ResolveCaptureRequest::NewStudy { board: fen::board_fen(&first), side: Color::Black })
        .unwrap();
    let CaptureState::Tracking { study_id, .. } = state else { panic!("{state:?}") };
    assert_ne!(study_id, sid);
    assert_eq!(service.study_view(&sid).unwrap().study.move_nodes().count(), 2);
    service.stop_capture();
    service.shutdown();
}

#[test]
fn explanation_reruns_after_settings_change() {
    if skip_engine_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start(dir.path(), rec.clone(), Arc::new(SimBackend::new()));
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("第一次详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");
    rec.clear();
    // 修改分析深度后，旧讲解的分析条件不再有效，再次查看时按新条件重新分析
    let mut cfg = service.status().config;
    cfg.analysis.quick_depth = 7;
    service.update_config(cfg).unwrap();
    service.explain_move(ExplainRequest { study_id: sid.clone(), node_id: node, budget: Budget::Quick, retry: false }).unwrap();
    let ev = rec.wait("按新设置重新讲解", |e, v| {
        e == "explanation_updated"
            && v["node_id"] == node
            && v["status"] == "detailed"
            && v["record"]["explanation"]["conditions"].as_str().is_some_and(|c| c.contains("深度≤7"))
    });
    assert_eq!(ev["record"]["budget"], "quick");
    service.shutdown();
}

/// 使用模拟引擎启动服务：把模拟引擎与脚本放到同一目录（引擎在自身目录下运行，默认读取该目录的脚本）。
fn start_with_mock(dir: &std::path::Path, script: &str, rec: Arc<Recorder>) -> AppService {
    let engine_dir = dir.join("engine");
    std::fs::create_dir_all(&engine_dir).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_xq-mock-uci"));
    let binary = engine_dir.join(exe.file_name().unwrap());
    std::fs::copy(&exe, &binary).unwrap();
    std::fs::write(engine_dir.join("mock-uci-script.txt"), script).unwrap();
    let mut cfg = xq_app::config::AppConfig::default();
    cfg.engine.binary = Some(binary);
    cfg.analysis.multipv = 2;
    cfg.analysis.quick_time_ms = 300;
    xq_app::config::save(&dir.join("config").join("config.json"), &cfg).unwrap();
    let paths = AppPaths {
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        log_dir: dir.join("logs"),
        resource_dir: repo_root(),
        legacy_config: None,
    };
    let service = AppService::start(paths, rec, Arc::new(SimBackend::new())).unwrap();
    wait_engine(&service);
    service
}

#[test]
fn failed_actual_move_analysis_is_partial_not_detailed() {
    // 候选搜索正常完成，但实际走法（马八进七）不在候选中，单独分析时引擎崩溃
    let script = "@go\n\
        info depth 1 seldepth 1 multipv 1 score cp 30 nodes 10 time 1 pv h2e2\n\
        info depth 1 seldepth 1 multipv 2 score cp 20 nodes 10 time 1 pv h0g2\n\
        bestmove h2e2\n\
        @go\n\
        exit 3\n";
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), script, rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("b0c2"), variation: false }).unwrap().node_id.unwrap();
    let ev = rec.wait("部分讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "partial");
    assert_eq!(ev["record"]["explanation"]["level"], "partial");
    assert!(ev["record"]["error"].as_str().unwrap().contains("实际着法单独分析失败"), "{ev}");
    assert_eq!(ev["stats"]["detailed"], 0);
    assert_eq!(ev["stats"]["partial"], 1);
    assert!(ev["record"]["explanation"]["verdict"].is_null(), "没有实际走法分析时不给出优劣结论");
    service.shutdown();
}

#[test]
fn late_quick_result_never_overwrites_deep() {
    // 第一次（快速）搜索等待 stop：深入分析请求会抢占它，被抢占的快速任务随后重新执行并晚于深入分析结束
    let round = "info depth 1 seldepth 1 multipv 1 score cp 30 nodes 10 time 1 pv h2e2\n\
        info depth 1 seldepth 1 multipv 2 score cp 20 nodes 10 time 1 pv h0g2\n\
        bestmove h2e2\n";
    let script = format!("@go\nwait-stop\n{round}@go\n{round}@go\n{round}");
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), &script, rec.clone());
    // 棋谱导入的着法以后台优先级排队讲解
    let created = service
        .create_study(CreateStudyRequest {
            name: None,
            kind: CreateKind::Record { text: "炮二平五".into() },
            orientation: None,
            user_side: None,
        })
        .unwrap();
    let sid = created.study.study.id.clone();
    let node = 1;
    std::thread::sleep(Duration::from_millis(300));
    service.explain_move(ExplainRequest { study_id: sid.clone(), node_id: node, budget: Budget::Deep, retry: false }).unwrap();
    rec.wait("深入分析完成", |e, v| {
        e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed" && v["record"]["budget"] == "deep"
    });
    // 等被抢占的快速任务重新执行完
    std::thread::sleep(Duration::from_millis(800));
    let detail = service.node_detail(NodeRef { study_id: sid.clone(), node_id: node }).unwrap();
    let record = detail.explanation.unwrap();
    assert_eq!(record.budget, Some(Budget::Deep), "较晚结束的快速分析不能覆盖深入分析");
    assert!(!record.pending);
    service.shutdown();
}

#[test]
fn retry_searches_again_instead_of_reusing_incomplete_analysis() {
    // 第一次搜索：深度 1 的完整一轮是 [炮二平五 +400, 马二进三 -100]，深度 2 只更新了第一名且最终着法变为马二进三，
    // 结果不完整。第二次搜索给出完整一致的候选。
    let script = "@go\n\
        info depth 1 seldepth 1 multipv 1 score cp 400 nodes 10 time 1 pv h2e2\n\
        info depth 1 seldepth 1 multipv 2 score cp -100 nodes 10 time 1 pv h0g2\n\
        info depth 2 seldepth 2 multipv 1 score cp 500 nodes 20 time 2 pv h0g2\n\
        bestmove h0g2\n\
        @go\n\
        info depth 1 seldepth 1 multipv 1 score cp 30 nodes 10 time 1 pv h2e2\n\
        info depth 1 seldepth 1 multipv 2 score cp 20 nodes 10 time 1 pv h0g2\n\
        bestmove h2e2\n";
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), script, rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h0g2"), variation: false }).unwrap().node_id.unwrap();
    let ev = rec.wait("部分讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "partial");
    let expl = &ev["record"]["explanation"];
    // 引擎最终选择的正是马二进三：不完整的候选不能把它判为失误
    assert_eq!(expl["verdict"]["tone"], "unclear", "{expl}");
    let comparison = expl["comparison"].to_string();
    assert!(!comparison.contains("严重失误") && !comparison.contains("低约"), "{comparison}");

    // 普通查看不重新分析部分完成的讲解
    rec.clear();
    let view = |retry| ExplainRequest { study_id: sid.clone(), node_id: node, budget: Budget::Quick, retry };
    service.explain_move(view(false)).unwrap();
    let rerun = rec.wait_for(Duration::from_millis(400), |e, v| e == "explanation_updated" && v["node_id"] == node);
    assert!(rerun.is_none(), "{rerun:?}");
    // 明确重试：不复用不完整的分析，引擎收到第二次搜索
    service.explain_move(view(true)).unwrap();
    let ev = rec.wait("重试后的详细讲解", |e, v| e == "explanation_updated" && v["node_id"] == node && v["status"] == "detailed");
    let expl = &ev["record"]["explanation"];
    assert_eq!(expl["level"], "detailed");
    assert_eq!(expl["verdict"]["tone"], "close", "{expl}");
    assert_eq!(expl["candidates"][0]["score"]["value"], 30, "使用第二次搜索的结果");
    service.shutdown();
}

#[test]
fn superseded_result_cannot_commit_after_newer_request() {
    // 第一次搜索：起始局面的深入分析（进入缓存）；第二次：炮二平五的快速讲解
    let script = "@go\n\
        info depth 1 seldepth 1 multipv 1 score cp 35 nodes 10 time 1 pv h2e2\n\
        info depth 1 seldepth 1 multipv 2 score cp 25 nodes 10 time 1 pv h0g2\n\
        bestmove h2e2\n\
        @go\n\
        info depth 1 seldepth 1 multipv 1 score cp 30 nodes 10 time 1 pv h2e2\n\
        info depth 1 seldepth 1 multipv 2 score cp 20 nodes 10 time 1 pv h0g2\n\
        bestmove h2e2\n";
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), script, rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    service
        .analyze_position(AnalyzeRequest { study_id: sid.clone(), node_id: ROOT, budget: Budget::Deep, request_id: "deep".into() })
        .unwrap();
    rec.wait("深入分析进入缓存", |e, v| e == "analysis_done" && v["request_id"] == "deep");

    // 快速讲解确认仍是最新请求后、写入前暂停
    let (paused_tx, paused_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(Some((paused_tx, resume_rx)));
    service.set_commit_hook(Some(Arc::new(move |_: &str, _, budget| {
        if budget == Budget::Quick
            && let Some((paused, resume)) = gate.lock().unwrap().take()
        {
            paused.send(()).unwrap();
            resume.recv().unwrap();
        }
    })));
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    paused_rx.recv_timeout(Duration::from_secs(30)).expect("快速讲解应进入提交");

    // 暂停期间提交深入讲解：它命中缓存，但必须等旧任务提交完毕才能登记与写入
    service.explain_move(ExplainRequest { study_id: sid.clone(), node_id: node, budget: Budget::Deep, retry: false }).unwrap();
    let is_deep = |e: &str, v: &Value| e == "explanation_updated" && v["node_id"] == node && v["record"]["budget"] == "deep";
    let early = rec.wait_for(Duration::from_millis(500), is_deep);
    assert!(early.is_none(), "旧任务提交期间不能插入新的写入：{early:?}");
    resume_tx.send(()).unwrap();
    rec.wait("深入讲解", |e, v| is_deep(e, v) && v["status"] == "detailed");
    let record = service.node_detail(NodeRef { study_id: sid.clone(), node_id: node }).unwrap().explanation.unwrap();
    assert_eq!(record.budget, Some(Budget::Deep), "旧的快速讲解不能覆盖深入讲解");
    assert!(!record.pending);
    service.shutdown();
}

#[test]
fn engine_unavailable_keeps_basic_explanation_with_reason() {
    // 资源目录里没有引擎：服务照常启动，摆棋、走子与规则讲解仍可用
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let paths = AppPaths {
        data_dir: dir.path().join("data"),
        config_dir: dir.path().join("config"),
        log_dir: dir.path().join("logs"),
        resource_dir: dir.path().join("no-resources"),
        legacy_config: None,
    };
    let service = AppService::start(paths, rec.clone(), Arc::new(SimBackend::new())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(service.status().engine, EngineState::Failed { .. }) {
        assert!(Instant::now() < deadline, "{:?}", service.status().engine);
        std::thread::sleep(Duration::from_millis(20));
    }
    let created = service
        .create_study(CreateStudyRequest {
            name: None,
            kind: CreateKind::Setup { board: "3k5/9/9/9/9/9/9/9/4R4/4K4".into(), side: Color::Red },
            orientation: None,
            user_side: None,
        })
        .unwrap();
    let sid = created.study.study.id.clone();
    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("e1d1"), variation: false }).unwrap().node_id.unwrap();
    let ev = rec.wait("基本讲解与原因", |e, v| e == "explanation_updated" && v["node_id"] == node && !v["record"]["pending"].as_bool().unwrap_or(true));
    assert_eq!(ev["status"], "partial");
    assert!(ev["record"]["explanation"]["summary"].as_str().unwrap().contains("将死对方"));
    assert!(ev["record"]["error"].as_str().unwrap().contains("引擎不可用"), "{ev}");
    let detail = service.node_detail(NodeRef { study_id: sid, node_id: node }).unwrap();
    assert_eq!(detail.status, GameStatus::Checkmate);
    service.shutdown();
}

#[test]
fn comparison_searches_both_moves_together_and_reuses_the_result() {
    // 对两步棋的同一次搜索：限定两步（searchmoves），每步一条变化
    let script = "@go\n\
        info depth 5 seldepth 5 multipv 1 score cp 40 nodes 10 time 1 pv h2e2 h9g7\n\
        info depth 5 seldepth 5 multipv 2 score cp -150 nodes 10 time 1 pv c3c4 h7e7\n\
        bestmove h2e2\n";
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), script, rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let request = |mine: &str, reference: Option<&str>| CompareRequest {
        study_id: sid.clone(),
        node_id: ROOT,
        mine: mv(mine),
        reference: reference.map(mv),
    };

    // 先返回规则事实，搜索完成后推送完整结果
    let first = service.compare_moves(request("c3c4", Some("h2e2"))).unwrap();
    assert!(first.pending);
    let facts = serde_json::to_value(&first).unwrap();
    assert_eq!(facts["comparison"]["outcome"], "not_analyzed");
    assert!(!facts["comparison"]["mine"]["changes"].as_array().unwrap().is_empty(), "规则事实不等待搜索");
    let done = rec.wait("对比完成", |e, v| e == "comparison_updated" && v["pending"] == false);
    assert_eq!(done["comparison"]["outcome"], "reference_better", "{done}");
    // 推送的完整结果带有请求时的序号与搜索条件，界面据此判断它属于哪一次请求
    assert_eq!(done["seq"], first.seq, "{done}");
    assert!(first.conditions_key.is_some());
    assert_eq!(done["conditions_key"].as_str(), first.conditions_key.as_deref());
    let text = done["comparison"]["differences"].to_string();
    assert!(text.contains("兵七进一比炮二平五低约 190 分"), "{text}");
    assert_eq!(done["comparison"]["mine"]["reply"]["text"], "对方最值得关注的回应是炮8平5");
    assert_eq!(done["comparison"]["reference"]["line"]["moves"][0], "h2e2", "对照的变化从比较起点开始");

    // 引擎收到的是同一次搜索：两步一起限定，两条变化
    let log = std::fs::read_to_string(dir.path().join("engine").join("mock-uci-commands.log")).unwrap();
    let gos: Vec<&str> = log.lines().filter(|l| l.starts_with("go")).collect();
    assert_eq!(gos.len(), 1, "{log}");
    assert!(gos[0].contains("searchmoves c3c4 h2e2"), "{log}");
    assert!(log.contains("setoption name MultiPV value 2"), "{log}");

    // 同样两步（不论哪一步是“我的选择”）直接使用缓存的结果，不再搜索
    let again = service.compare_moves(request("h2e2", Some("c3c4"))).unwrap();
    assert!(!again.pending);
    assert!(again.seq > first.seq, "每次请求的序号递增，较新的请求直接返回的结果也能被识别为较新");
    assert_eq!(serde_json::to_value(&again).unwrap()["comparison"]["outcome"], "mine_better");
    let log = std::fs::read_to_string(dir.path().join("engine").join("mock-uci-commands.log")).unwrap();
    assert_eq!(log.lines().filter(|l| l.starts_with("go")).count(), 1, "{log}");

    // 两步相同、着法不合法，或没有可以对照的着法时给出说明
    assert!(service.compare_moves(request("h2e2", Some("h2e2"))).is_err());
    assert!(service.compare_moves(request("h2e2", Some("a0a5"))).is_err());
    let none = service.compare_moves(request("h2e2", None)).unwrap_err();
    assert!(none.message.contains("没有可以对照的着法"), "{}", none.message);
    service.shutdown();
}

#[test]
fn think_hints_come_with_the_position_and_practice_is_recorded_on_the_move() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    // 落子前局面的详情带有分级提示（不依赖棋谱中的实际着法）
    let detail = serde_json::to_value(service.node_detail(NodeRef { study_id: sid.clone(), node_id: ROOT }).unwrap()).unwrap();
    assert_eq!(detail["hints"]["topic"], "development", "{}", detail["hints"]);
    assert!(detail["hints"]["observe"].as_str().unwrap().contains("没有出动的子力"));

    let node = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    // 保存的是界面记下的、练习时实际看到的提示，不按提交时的分析重新生成；只保留用到的两级
    let shown = UsedHints {
        observe: "练习时看到的一级提示".into(),
        focus: Some(HintFocus { text: "练习时看到的二级提示".into(), squares: vec![], arrows: vec![] }),
        moves: vec![HintMove { mv: mv("b0c2"), notation: "马八进七".into() }],
    };
    let view = service
        .record_practice(PracticeRequest {
            study_id: sid.clone(),
            node_id: node,
            hints_used: 2,
            revealed_directly: false,
            first_choice: Some(mv("h2e2")),
            hints: Some(shown),
        })
        .unwrap();
    let practice = serde_json::to_value(&view.study.nodes[&node].practice).unwrap();
    assert_eq!((practice["hints_used"].as_u64(), practice["matched"].as_bool(), practice["sessions"].as_u64()), (Some(2), Some(true), Some(1)));
    assert_eq!(practice["hints"]["observe"], "练习时看到的一级提示");
    assert_eq!(practice["hints"]["focus"]["text"], "练习时看到的二级提示");
    assert_eq!(practice["hints"]["moves"].as_array().unwrap().len(), 0, "没有用到三级提示：{practice}");
    // 练习记录随会话保存，重新打开仍在
    let reopened = service.load_study(&sid).unwrap();
    assert!(reopened.study.nodes[&node].practice.is_some());
    service.shutdown();
}

#[test]
fn review_card_keeps_the_position_and_the_thinking_and_survives_the_study() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: Some("复习测试".into()), kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    let second = service.apply_move(MoveRequest { study_id: sid.clone(), parent: first, mv: mv("h9g7"), variation: false }).unwrap().node_id.unwrap();
    // 先想后讲：用了两级提示（界面记下的是落子前局面详情中的提示），第一步走了马2进3
    let shown = service.node_detail(NodeRef { study_id: sid.clone(), node_id: first }).unwrap().hints.unwrap().used(3);
    service
        .record_practice(PracticeRequest {
            study_id: sid.clone(),
            node_id: second,
            hints_used: 2,
            revealed_directly: false,
            first_choice: Some(mv("b9c7")),
            hints: shown,
        })
        .unwrap();
    service.set_note(NoteRequest { study_id: sid.clone(), node_id: second, note: "为什么不走马2进3？".into(), starred: true }).unwrap();
    rec.wait("第二着的讲解完成", |e, v| e == "explanation_updated" && v["node_id"] == second && v["record"]["pending"] == false);

    let item = service.add_review(NodeRef { study_id: sid.clone(), node_id: second }).unwrap();
    let card = &item.card;
    let before = service.node_detail(NodeRef { study_id: sid.clone(), node_id: first }).unwrap();
    assert_eq!(card.fen, format!("{} b", before.node.fen.split(' ').next().unwrap()), "卡片保存落子前的局面与行棋方");
    assert_eq!((card.side, card.turn, card.status), (Color::Black, 1, ReviewStatus::Due));
    assert_eq!(card.history.iter().map(|m| m.notation.as_str()).collect::<Vec<_>>(), ["炮二平五"], "保存此前的着法");
    assert!(card.history_complete);
    assert_eq!(card.answer.notation, "马8进7");
    let thinking = card.thinking.as_ref().expect("保存当时的练习记录");
    assert_eq!(thinking.first_choice.as_ref().map(|m| m.notation.as_str()), Some("马2进3"));
    let hints = thinking.hints.as_ref().expect("保存当时看过的提示原文");
    assert!(!hints.observe.is_empty() && hints.focus.is_some() && hints.moves.is_empty(), "只保留用到的两级提示：{hints:?}");
    assert_eq!(card.note, "为什么不走马2进3？", "加入时带入这一着的笔记");
    let basis = card.basis.as_ref().expect("保存程序讲解");
    assert!(!basis.summary.is_empty() && basis.generator_version > 0);
    assert!(item.source_available);

    // 再次加入不覆盖原来的卡片；这一着的详情显示复习状态
    let again = service.add_review(NodeRef { study_id: sid.clone(), node_id: second }).unwrap();
    assert_eq!(again.card.created_at, card.created_at);
    assert_eq!(service.node_detail(NodeRef { study_id: sid.clone(), node_id: second }).unwrap().review, Some(ReviewStatus::Due));
    assert!(service.add_review(NodeRef { study_id: sid.clone(), node_id: ROOT }).is_err(), "起始局面不能加入复习");

    // 复习：先给出局面与合法着法，重新选择后记下是否与棋谱相同
    let id = card.id.clone();
    let detail = service.review_detail(ReviewRef { id: id.clone() }).unwrap();
    assert!(detail.legal_moves.contains(&mv("h9g7")) && detail.legal_moves.contains(&mv("b9c7")));
    let err = service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: Some(mv("h2e2")), goal: PracticeGoal::Recall, intent: None }).unwrap_err();
    assert!(err.message.contains("不合规则"), "{}", err.message);
    let tried = service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: Some(mv("h9g7")), goal: PracticeGoal::Recall, intent: None }).unwrap();
    assert!(tried.card.attempts.last().unwrap().matched);
    let tried = service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: None, goal: PracticeGoal::Recall, intent: None }).unwrap();
    assert_eq!(tried.card.attempts.len(), 2);
    assert!(!tried.card.attempts[1].matched, "直接看答案不算走对");
    assert!(tried.card.reviewed_at.is_some());
    let updated = service
        .update_review(ReviewUpdateRequest { id: id.clone(), status: Some(ReviewStatus::Again), note: Some("马8进7保护中卒".into()), issues: None })
        .unwrap();
    assert_eq!((updated.card.status, updated.card.note.as_str()), (ReviewStatus::Again, "马8进7保护中卒"));

    // 列表按下一次复习的时间：新加入的卡片立即到期，排在明天再看的“需要再练”之前
    service.add_review(NodeRef { study_id: sid.clone(), node_id: first }).unwrap();
    let list = service.list_reviews().unwrap();
    assert_eq!(list.iter().map(|i| i.card.status).collect::<Vec<_>>(), [ReviewStatus::Due, ReviewStatus::Again]);

    // 重新启动后仍在；删除会话后卡片保留，仍可复习，只是不能打开原局面
    service.shutdown();
    drop(service);
    let service = start_with_mock(dir.path(), "", rec.clone());
    assert_eq!(service.list_reviews().unwrap().len(), 2);
    service.delete_study(&sid).unwrap();
    let list = service.list_reviews().unwrap();
    assert!(list.iter().all(|i| !i.source_available && i.study_name == "复习测试"), "原会话删除后保留卡片");
    let detail = service.review_detail(ReviewRef { id: id.clone() }).unwrap();
    assert_eq!(detail.item.card.answer.notation, "马8进7");
    service.delete_review(ReviewRef { id: id.clone() }).unwrap();
    assert_eq!(service.list_reviews().unwrap().len(), 1);
    assert!(service.review_detail(ReviewRef { id }).is_err());
    service.shutdown();
}

#[test]
fn decision_practice_compares_the_choice_in_one_search_and_records_the_intent() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: Some("局面决策".into()), kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    let second = service.apply_move(MoveRequest { study_id: sid.clone(), parent: first, mv: mv("h9g7"), variation: false }).unwrap().node_id.unwrap();
    for node in [first, second] {
        rec.wait("讲解完成", |e, v| e == "explanation_updated" && v["node_id"] == node && v["record"]["pending"] == false);
    }
    let id = service.add_review(NodeRef { study_id: sid.clone(), node_id: second }).unwrap().card.id;

    // 讲解的搜索都已结束；按复习的搜索顺序换上脚本并重启引擎：
    // 多候选搜索找出引擎参考（马8进7）；之后每次是对这次的选择、棋谱着法与引擎参考的同一次搜索
    let script = "@go\n\
        info depth 5 seldepth 5 multipv 1 score cp 10 nodes 10 time 1 pv h9g7 h0g2\n\
        info depth 5 seldepth 5 multipv 2 score cp 0 nodes 10 time 1 pv b9c7 h0g2\n\
        bestmove h9g7\n\
        @go\n\
        info depth 5 seldepth 5 multipv 1 score cp 10 nodes 10 time 1 pv h9g7 h0g2\n\
        info depth 5 seldepth 5 multipv 2 score cp 0 nodes 10 time 1 pv b9c7 h0g2\n\
        bestmove h9g7\n\
        @go\n\
        info depth 5 seldepth 5 multipv 1 score cp 10 nodes 10 time 1 pv h9g7 h0g2\n\
        info depth 5 seldepth 5 multipv 2 score cp -80 nodes 10 time 1 pv i6i5 e2e6\n\
        bestmove h9g7\n\
        @go\n\
        info depth 5 seldepth 5 multipv 1 score cp 10 nodes 10 time 1 pv h9g7 h0g2\n\
        bestmove h9g7\n";
    std::fs::write(dir.path().join("engine").join("mock-uci-script.txt"), script).unwrap();
    service.restart_engine();
    std::thread::sleep(Duration::from_millis(300));
    wait_engine(&service);
    let attempt = |choice: &str, goal: PracticeGoal, intent: Option<Intent>| {
        rec.clear();
        let item = service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: Some(mv(choice)), goal, intent }).unwrap();
        item.card.attempts.last().unwrap().clone()
    };
    let decided = |what: &str| {
        let v = rec.wait(what, |e, v| {
            e == "review_updated" && v["card"]["attempts"].as_array().unwrap().last().unwrap()["decision"]["state"] != "pending"
        });
        v["card"]["attempts"].as_array().unwrap().last().unwrap()["decision"].clone()
    };

    // 与棋谱不同、但与参考评分接近：意图与这一步实际的规则事实同时记下
    let a = attempt("b9c7", PracticeGoal::Decision, Some(Intent::Develop));
    assert_eq!(a.decision.as_ref().unwrap().state, DecisionState::Pending, "比较在后台进行");
    let reflection = a.reflection.unwrap();
    assert_eq!(reflection.checks, vec!["这一步把右马从底线走出"]);
    assert!(!reflection.changes.is_empty());
    let d = decided("接近参考");
    assert_eq!(d["verdict"], "reasonable", "{d}");
    assert_eq!(d["reference"]["notation"], "马8进7");
    assert!(d["points"][0].as_str().unwrap().contains("评分接近"), "{d}");
    assert!(d["conditions"].as_str().unwrap().contains("指定着法单独分析"));

    // 明显低于参考：留下值得注意的问题，并给出对方的回应（多候选搜索的结果已缓存，不再搜索）
    attempt("i6i5", PracticeGoal::Decision, None);
    let d = decided("值得注意");
    assert_eq!(d["verdict"], "noteworthy", "{d}");
    assert!(d["points"][1].as_str().unwrap().contains("对方最值得关注的回应是炮五进四"), "{d}");

    // 复现棋谱：只看是否与棋谱相同，不搜索
    let recall = attempt("b9c7", PracticeGoal::Recall, None);
    assert!(recall.decision.is_none() && !recall.matched);
    // 与棋谱一致
    attempt("h9g7", PracticeGoal::Decision, None);
    assert_eq!(decided("与棋谱一致")["verdict"], "same_as_record");
    let log = std::fs::read_to_string(dir.path().join("engine").join("mock-uci-commands.log")).unwrap();
    assert!(log.contains("searchmoves b9c7 h9g7") && log.contains("searchmoves h9g7 i6i5"), "{log}");

    // 退出时还在进行的比较，重新启动后标为没有得到结果（同一组着法的搜索已有缓存，这里直接写入进行中的状态）
    service.shutdown();
    drop(service);
    {
        let store = xq_app::storage::Store::open(&dir.path().join("data").join("xqlink.redb")).unwrap();
        store
            .update_review(&id, |c| c.attempts.last_mut().unwrap().decision.as_mut().unwrap().state = DecisionState::Pending)
            .unwrap();
    }
    let service = start_with_mock(dir.path(), "", rec.clone());
    let card = service.review_detail(ReviewRef { id }).unwrap().item.card;
    let last = card.attempts.last().unwrap().decision.as_ref().unwrap();
    assert_eq!(last.state, DecisionState::Failed);
    assert!(last.error.as_deref().unwrap().contains("退出前没有完成"));
    service.shutdown();
}

#[test]
fn marking_a_card_schedules_the_next_review_from_the_latest_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: Some("复习安排".into()), kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    let id = service.add_review(NodeRef { study_id: sid.clone(), node_id: first }).unwrap().card.id;
    let day = xq_app::schedule::DAY_MS;
    let attempt = |choice: Option<&str>| {
        service
            .review_attempt(ReviewAttemptRequest { id: id.clone(), choice: choice.map(mv), goal: PracticeGoal::Recall, intent: None })
            .unwrap();
    };
    let mark = |status: ReviewStatus| {
        let before = now();
        let card = service.update_review(ReviewUpdateRequest { id: id.clone(), status: Some(status), note: None, issues: None }).unwrap().card;
        (card.stage, (card.due_at.unwrap() - before + day / 2) / day)
    };
    // 新卡片立即到期
    let fresh = service.list_reviews().unwrap().into_iter().find(|i| i.card.id == id).unwrap().card;
    assert!(fresh.due_at.unwrap() <= now());
    // 独立选对后标为已理解：升一档（3 天）；同一次复习再标一次不再升档，到期时间也不变
    attempt(Some("h2e2"));
    assert_eq!(mark(ReviewStatus::Understood), (1, 3));
    let due = |svc: &AppService| svc.list_reviews().unwrap().into_iter().find(|i| i.card.id == id).unwrap().card.due_at;
    let first_due = due(&service);
    assert_eq!(mark(ReviewStatus::Understood), (1, 3));
    assert_eq!(due(&service), first_due, "同一次复习重复标记，安排不变");
    // 有了新的复习才继续升档（7 天）
    attempt(Some("h2e2"));
    assert_eq!(mark(ReviewStatus::Understood), (2, 7));
    // 直接看答案后标为已理解：档位不升，明天再看
    attempt(None);
    assert_eq!(mark(ReviewStatus::Understood), (2, 1));
    // 需要再练：回到第一档
    attempt(Some("b0c2"));
    assert_eq!(mark(ReviewStatus::Again), (0, 1));
    // 设置中的档位
    let mut cfg = service.status().config;
    cfg.review.intervals_days = vec![2, 5];
    service.update_config(cfg).unwrap();
    attempt(Some("h2e2"));
    assert_eq!(mark(ReviewStatus::Understood), (1, 5));
    service.shutdown();
}

#[test]
fn learning_archive_restores_studies_and_cards_and_merges_reviews() {
    let rec = Arc::new(Recorder::default());
    let home = tempfile::tempdir().unwrap();
    let service = start_with_mock(home.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: Some("档案".into()), kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    let id = service.add_review(NodeRef { study_id: sid.clone(), node_id: first }).unwrap().card.id;
    service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: Some(mv("h2e2")), goal: PracticeGoal::Recall, intent: None }).unwrap();
    service.update_review(ReviewUpdateRequest { id: id.clone(), status: Some(ReviewStatus::Understood), note: Some("架中炮".into()), issues: None }).unwrap();
    let archive = service.export_archive().unwrap();

    // 另一台设备：预览后导入，会话保留原来的编号，卡片仍能打开原局面
    let other = tempfile::tempdir().unwrap();
    let restored = start_with_mock(other.path(), "", rec.clone());
    let text = |t: &str| ArchiveSource::Text { text: t.to_string() };
    let preview = restored.preview_archive(text(&archive)).unwrap();
    assert_eq!((preview.studies_new, preview.cards_new, preview.cards_merge), (1, 1, 0));
    assert!(restored.list_reviews().unwrap().is_empty(), "预览不改动本机的数据");
    restored.import_archive(text(&archive)).unwrap();
    let item = restored.list_reviews().unwrap().pop().unwrap();
    assert!(item.source_available, "原会话一起恢复，编号不变");
    assert_eq!((item.card.note.as_str(), item.card.status, item.card.attempts.len()), ("架中炮", ReviewStatus::Understood, 1));
    assert_eq!(item.card.due_at, service.list_reviews().unwrap()[0].card.due_at, "下一次复习的安排一起恢复");
    assert!(restored.load_study(&sid).is_ok());
    // 再次导入：已有的会话保留本机版本，卡片相同
    let again = restored.preview_archive(text(&archive)).unwrap();
    assert_eq!((again.studies_new, again.studies_existing, again.cards_new, again.cards_same), (0, 1, 0, 1));

    // 原设备之后又复习了一次；导入到另一台设备时合并历次复习
    service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: None, goal: PracticeGoal::Recall, intent: None }).unwrap();
    restored.update_review(ReviewUpdateRequest { id: id.clone(), status: None, note: Some("另一台设备上的理解".into()), issues: None }).unwrap();
    let newer = service.export_archive().unwrap();
    assert_eq!(restored.preview_archive(text(&newer)).unwrap().cards_merge, 1);
    restored.import_archive(text(&newer)).unwrap();
    let merged = restored.list_reviews().unwrap().pop().unwrap().card;
    assert_eq!(merged.attempts.len(), 2, "历次复习合在一起");

    // 原会话已删除的卡片照样导入，只是不能打开原局面
    service.delete_study(&sid).unwrap();
    let orphan = service.export_archive().unwrap();
    let third = tempfile::tempdir().unwrap();
    let fresh = start_with_mock(third.path(), "", rec.clone());
    assert_eq!(fresh.preview_archive(text(&orphan)).unwrap().cards_orphan, 1);
    fresh.import_archive(text(&orphan)).unwrap();
    let item = fresh.list_reviews().unwrap().pop().unwrap();
    assert!(!item.source_available && item.study_name == "档案");
    assert!(fresh.review_detail(ReviewRef { id: id.clone() }).is_ok(), "仍可复习");
    // 不是学习档案
    assert!(fresh.preview_archive(text("{\"format\":\"xqlink-study\"}")).is_err());

    // 立即备份；有学习数据时，下次启动自动备份当天的一份
    let info = service.backup_now().unwrap();
    assert_eq!(info.files.len(), 1);
    assert!(info.files[0].name.starts_with("xqlink-archive-") && info.files[0].size > 0);
    fresh.shutdown();
    drop(fresh);
    std::fs::remove_dir_all(third.path().join("data").join("backups")).ok();
    let fresh = start_with_mock(third.path(), "", rec.clone());
    assert_eq!(fresh.backup_info().files.len(), 1, "启动时自动备份");
    for s in [service, restored, fresh] {
        s.shutdown();
    }
}

fn now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64 }

#[test]
fn game_report_picks_key_positions_from_practice_and_marks() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: Some("整局报告".into()), kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let apply = |parent: xq_core::study::NodeId, m: &str, variation: bool| {
        service.apply_move(MoveRequest { study_id: sid.clone(), parent, mv: mv(m), variation }).unwrap().node_id.unwrap()
    };
    let first = apply(ROOT, "h2e2", false);
    let second = apply(first, "h9g7", false);
    let third = apply(second, "h0g2", false);
    // 先想后讲：第 1 着走对；第 2 着用了两级提示、走了马2进3（试走分支）；第 3 着收藏
    let practice = |node: xq_core::study::NodeId, hints: u8, choice: &str| {
        service
            .record_practice(PracticeRequest {
                study_id: sid.clone(),
                node_id: node,
                hints_used: hints,
                revealed_directly: false,
                first_choice: Some(mv(choice)),
                hints: None,
            })
            .unwrap();
    };
    practice(first, 0, "h2e2");
    apply(first, "b9c7", true);
    practice(second, 2, "b9c7");
    service.set_note(NoteRequest { study_id: sid.clone(), node_id: third, note: String::new(), starred: true }).unwrap();
    service.add_review(NodeRef { study_id: sid.clone(), node_id: third }).unwrap();

    let report = service.game_report(&sid).unwrap();
    assert_eq!((report.moves, report.practice.practiced, report.practice.matched, report.practice.hinted), (3, 2, 1, 1));
    let keys: Vec<_> = report.key_positions.iter().map(|k| (k.node_id, k.label.as_str())).collect();
    assert_eq!(keys, [(second, "第 1 回合 · 黑方 马8进7"), (third, "第 2 回合 · 红方 马二进三")], "按着法顺序");
    let reasons = &report.key_positions[0].reasons;
    assert!(reasons.contains(&"先想后讲：第一步走了马2进3，与棋谱不同".to_string()) && reasons.contains(&"先想后讲：用了 2 级提示".to_string()), "{reasons:?}");
    assert_eq!(report.key_positions[1].reasons, ["收藏"]);
    assert_eq!(report.key_positions[1].review, Some(ReviewStatus::Due), "已在复习中的局面标明状态");
    assert_eq!(report.good_choices.len(), 1);
    assert_eq!(report.good_choices[0].text, "先想后讲中第一步与棋谱一致：炮二平五");
    // 没有完整的引擎分析时如实说明，也不按评价入选
    assert!(report.notes.iter().any(|n| n.contains("主线 3 着中 0 着有完整的引擎分析")), "{:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.contains("关键局面只有 2 处")));

    // 卡片附带程序根据规则事实建议的主题；用户确认的问题整组保存（去重、按固定顺序）
    let item = service.list_reviews().unwrap().pop().unwrap();
    assert!(item.themes.iter().any(|t| t.theme == Theme::Protection && t.why.starts_with("这一步保护了")), "{:?}", item.themes);
    let id = item.card.id;
    let issues = vec![Issue::MissedReply, Issue::MissedThreat, Issue::MissedThreat];
    let card = service.update_review(ReviewUpdateRequest { id, status: None, note: None, issues: Some(issues) }).unwrap().card;
    assert_eq!(card.issues, [Issue::MissedThreat, Issue::MissedReply]);
    service.shutdown();
}

#[test]
fn a_recalled_record_move_that_the_search_shows_worse_is_not_kept_as_a_good_choice() {
    // 第 1 着炮二平五：多候选搜索的首选是马二进三，实际着法单独分析明显更低
    let script = "@go\n\
        info depth 5 seldepth 5 multipv 1 score cp 200 nodes 10 time 1 pv h0g2 h9g7\n\
        info depth 5 seldepth 5 multipv 2 score cp 190 nodes 10 time 1 pv b0c2 h9g7\n\
        bestmove h0g2\n\
        @go\n\
        info depth 5 seldepth 5 multipv 1 score cp 50 nodes 10 time 1 pv h2e2 h9g7\n\
        bestmove h2e2\n";
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), script, rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    rec.wait("完整讲解", |e, v| e == "explanation_updated" && v["node_id"] == first && v["status"] == "detailed");
    service
        .record_practice(PracticeRequest {
            study_id: sid.clone(),
            node_id: first,
            hints_used: 0,
            revealed_directly: false,
            first_choice: Some(mv("h2e2")),
            hints: None,
        })
        .unwrap();
    let report = service.game_report(&sid).unwrap();
    assert!(report.good_choices.is_empty(), "回忆对了棋谱着法，但它已被评价为明显不如首选：{:?}", report.good_choices);
    let key = &report.key_positions[0];
    assert!(key.reasons.iter().any(|r| r == "实际着法明显不如首选（比首选低约 150 分）"), "{:?}", key.reasons);
    assert_eq!(report.practice.matched, 1, "统计中仍记为第一步与棋谱一致");
    service.shutdown();
}

#[test]
fn a_mark_made_before_the_decision_result_is_rescheduled_when_the_result_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let service = start_with_mock(dir.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    let second = service.apply_move(MoveRequest { study_id: sid.clone(), parent: first, mv: mv("h9g7"), variation: false }).unwrap().node_id.unwrap();
    for node in [first, second] {
        rec.wait("讲解完成", |e, v| e == "explanation_updated" && v["node_id"] == node && v["record"]["pending"] == false);
    }
    let id = service.add_review(NodeRef { study_id: sid.clone(), node_id: second }).unwrap().card.id;
    // 多候选搜索找出引擎参考；同一次比较在返回前暂停，好在结果出来之前标记状态
    let script = "@go\n\
        info depth 5 seldepth 5 multipv 1 score cp 10 nodes 10 time 1 pv h9g7 h0g2\n\
        info depth 5 seldepth 5 multipv 2 score cp 0 nodes 10 time 1 pv b9c7 h0g2\n\
        bestmove h9g7\n\
        @go\n\
        sleep 1500\n\
        info depth 5 seldepth 5 multipv 1 score cp 10 nodes 10 time 1 pv h9g7 h0g2\n\
        info depth 5 seldepth 5 multipv 2 score cp 0 nodes 10 time 1 pv b9c7 h0g2\n\
        bestmove h9g7\n";
    std::fs::write(dir.path().join("engine").join("mock-uci-script.txt"), script).unwrap();
    service.restart_engine();
    std::thread::sleep(Duration::from_millis(300));
    wait_engine(&service);
    rec.clear();
    let tried = service
        .review_attempt(ReviewAttemptRequest { id: id.clone(), choice: Some(mv("b9c7")), goal: PracticeGoal::Decision, intent: None })
        .unwrap();
    let at = tried.card.attempts.last().unwrap().at;
    // 比较还没有结果：先按不升档安排，不当作选错
    let marked = service.update_review(ReviewUpdateRequest { id: id.clone(), status: Some(ReviewStatus::Understood), note: None, issues: None }).unwrap().card;
    assert_eq!(marked.attempts.last().unwrap().decision.as_ref().unwrap().state, DecisionState::Pending);
    let base = marked.schedule_base.unwrap();
    assert_eq!((base.attempt_at, marked.stage), (Some(at), 0));
    // 结果回来（与参考评分接近）：从同一个起点重新安排，升一档
    let done = rec.wait("比较完成", |e, v| {
        e == "review_updated" && v["card"]["attempts"].as_array().unwrap().last().unwrap()["decision"]["state"] == "ready"
    });
    assert_eq!(done["card"]["attempts"].as_array().unwrap().last().unwrap()["decision"]["support"], "supported");
    let card = service.list_reviews().unwrap().into_iter().find(|i| i.card.id == id).unwrap().card;
    assert_eq!(card.stage, 1);
    assert_eq!(card.due_at, Some(base.at + 3 * xq_app::schedule::DAY_MS), "按第一次标记的时间计算");
    // 同一次复习再标一次：不再升档
    let again = service.update_review(ReviewUpdateRequest { id, status: Some(ReviewStatus::Understood), note: None, issues: None }).unwrap().card;
    assert_eq!((again.stage, again.due_at), (1, card.due_at));
    service.shutdown();
}


#[test]
fn imported_comparisons_that_were_still_running_are_queued_again() {
    let rec = Arc::new(Recorder::default());
    let home = tempfile::tempdir().unwrap();
    let service = start_with_mock(home.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    let id = service.add_review(NodeRef { study_id: sid.clone(), node_id: first }).unwrap().card.id;
    service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: Some(mv("h2e2")), goal: PracticeGoal::Recall, intent: None }).unwrap();
    // 档案中的这次复习是局面决策，导出时比较还在进行
    let mut archive: serde_json::Value = serde_json::from_str(&service.export_archive().unwrap()).unwrap();
    let attempt = &mut archive["reviews"][0]["attempts"][0];
    attempt["goal"] = serde_json::json!("decision");
    attempt["decision"] = serde_json::json!({
        "state": "pending", "verdict": null, "support": "unknown", "reference": null, "points": [], "conditions": null, "error": null
    });
    let other = tempfile::tempdir().unwrap();
    let restored = start_with_mock(other.path(), "", rec.clone());
    restored.import_archive(ArchiveSource::Text { text: archive.to_string() }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let state = loop {
        let card = restored.review_detail(ReviewRef { id: id.clone() }).unwrap().item.card;
        let state = card.attempts[0].decision.as_ref().unwrap().state;
        if state != DecisionState::Pending || Instant::now() > deadline {
            break state;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_ne!(state, DecisionState::Pending, "导入后重新排队，不会一直停在进行中");
    for s in [service, restored] {
        s.shutdown();
    }
}

#[test]
fn a_late_comparison_queued_by_an_earlier_import_does_not_overwrite_an_imported_result() {
    let rec = Arc::new(Recorder::default());
    let home = tempfile::tempdir().unwrap();
    let service = start_with_mock(home.path(), "", rec.clone());
    let created = service
        .create_study(CreateStudyRequest { name: None, kind: CreateKind::Standard, orientation: None, user_side: None })
        .unwrap();
    let sid = created.study.study.id.clone();
    let first = service.apply_move(MoveRequest { study_id: sid.clone(), parent: ROOT, mv: mv("h2e2"), variation: false }).unwrap().node_id.unwrap();
    let id = service.add_review(NodeRef { study_id: sid.clone(), node_id: first }).unwrap().card.id;
    service.review_attempt(ReviewAttemptRequest { id: id.clone(), choice: Some(mv("h2e2")), goal: PracticeGoal::Recall, intent: None }).unwrap();
    // 档案 P：这次局面决策导出时比较还在进行
    let mut pending: Value = serde_json::from_str(&service.export_archive().unwrap()).unwrap();
    let attempt = &mut pending["reviews"][0]["attempts"][0];
    attempt["goal"] = serde_json::json!("decision");
    attempt["decision"] = serde_json::json!({
        "state": "pending", "verdict": null, "support": "unknown", "reference": null, "points": [], "conditions": null, "error": null
    });
    let at = attempt["at"].as_i64().unwrap();
    // 档案 R：原设备上这次比较之后完成，并按结果标为已理解
    let mut ready = pending.clone();
    let card = &mut ready["reviews"][0];
    card["attempts"][0]["decision"] = serde_json::json!({
        "state": "ready", "verdict": "same_as_record", "support": "supported",
        "reference": { "mv": "h2e2", "notation": "炮二平五" }, "points": [], "conditions": "快速分析", "error": null
    });
    let base_at = card["updated_at"].as_i64().unwrap() + 1_000;
    let due = base_at + 3 * xq_app::schedule::DAY_MS;
    card["status"] = serde_json::json!("understood");
    card["stage"] = serde_json::json!(1);
    card["schedule_base"] = serde_json::json!({ "attempt_at": at, "stage": 0, "at": base_at });
    card["due_at"] = serde_json::json!(due);
    card["updated_at"] = serde_json::json!(base_at);

    // 另一台设备：导入 P 后重新排队的比较先暂停，之后没有得到完整的结果
    let other = tempfile::tempdir().unwrap();
    let script = "@go\nsleep 1500\ninfo depth 5 seldepth 5 multipv 1 score cp 10 nodes 10 time 1 pv h2e2 h9g7\nbestmove h2e2\n";
    let restored = start_with_mock(other.path(), script, rec.clone());
    restored.import_archive(ArchiveSource::Text { text: pending.to_string() }).unwrap();
    // 比较还没有结束时导入 R：完成的结果与安排恢复
    restored.import_archive(ArchiveSource::Text { text: ready.to_string() }).unwrap();
    let card_of = || restored.list_reviews().unwrap().into_iter().find(|i| i.card.id == id).unwrap().card;
    let imported = card_of();
    let d = imported.attempts[0].decision.as_ref().unwrap();
    assert_eq!((d.state, d.support), (DecisionState::Ready, xq_app::decision::DecisionSupport::Supported));
    assert_eq!((imported.status, imported.stage, imported.due_at), (ReviewStatus::Understood, 1, Some(due)));
    rec.clear();
    // 放行先前排队的比较：等它的第二次搜索（限定着法）发出并结束
    let log = other.path().join("engine").join("mock-uci-commands.log");
    let deadline = Instant::now() + Duration::from_secs(20);
    while std::fs::read_to_string(&log).unwrap_or_default().lines().filter(|l| l.starts_with("go")).count() < 2 {
        assert!(Instant::now() < deadline, "先前排队的比较没有继续");
        std::thread::sleep(Duration::from_millis(50));
    }
    let late = rec.wait_for(Duration::from_secs(2), |e, v| e == "review_updated" && v["card"]["id"] == id.as_str());
    assert!(late.is_none(), "迟到的结果没有写入，也不推送：{late:?}");
    assert_eq!(card_of(), imported, "完成的比较结果、是否支持这个选择与复习安排都不变");
    for s in [service, restored] {
        s.shutdown();
    }
}
