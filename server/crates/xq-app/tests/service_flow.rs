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
use xq_app::engine::EngineState;
use xq_app::events::EventSink;
use xq_app::records::Budget;
use xq_core::Board;
use xq_core::Color;
use xq_core::Move;
use xq_core::capture::Rect;
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
}

fn repo_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..") }

fn engine_available() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64")) && repo_root().join("libs/pikafish/pikafish-linux").exists()
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
    let deadline = Instant::now() + Duration::from_secs(60);
    while !matches!(service.status().engine, EngineState::Ready { .. }) {
        assert!(Instant::now() < deadline, "{:?}", service.status().engine);
        std::thread::sleep(Duration::from_millis(50));
    }
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
    if !engine_available() {
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
    if !engine_available() {
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
    if !engine_available() {
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
    if !engine_available() {
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
