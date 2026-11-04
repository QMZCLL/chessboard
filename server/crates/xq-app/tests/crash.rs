//! 崩溃一致性：子进程执行一项写入，在指定的注入点直接终止（不运行析构，不提交进行中的事务，模拟断电或强制结束）；
//! 父进程重新打开数据，检查只能是完整的旧状态（之后照常完成这项写入即得到完整的新状态），不会出现写了一半的状态。
//!
//! 子进程是这个测试程序本身：带上 `XQ_CRASH_SCENARIO` 等环境变量只运行 `crash_child`。注入点在子进程启动服务之后才打开
//! （迁移的注入点在启动前打开），启动时的其他写入不会提前触发。

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use redb::Database;
use redb::ReadableTable;
use redb::TableDefinition;
use serde_json::Value;
use serde_json::json;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::api::*;
use xq_app::archive::ArchiveSource;
use xq_app::capture::SimBackend;
use xq_app::decision::PracticeGoal;
use xq_app::engine::EngineState;
use xq_app::events::EventSink;
use xq_app::failpoints;
use xq_app::inbox::InboxImportGame;
use xq_app::inbox::InboxImportRequest;
use xq_app::review::ReviewAttemptRequest;
use xq_core::Color;
use xq_core::Move;
use xq_core::study::ROOT;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: &str, _payload: Value) {}
}

fn mv(s: &str) -> Move { Move::parse_iccs(s).unwrap() }

fn repo_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap() }

fn paths(dir: &Path) -> AppPaths {
    AppPaths {
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        log_dir: dir.join("logs"),
        resource_dir: repo_root(),
        legacy_config: None,
    }
}

/// 写入设置：`engine` 为 true 时使用仓库自带的 Pikafish（单线程、固定深度），否则引擎指向不存在的程序（不做新的分析）。
fn configure(dir: &Path, engine: bool) {
    let mut cfg = if engine {
        json!({ "version": 1, "engine": { "threads": 1, "hash_mb": 16 }, "analysis": { "quick_depth": 6, "quick_time_ms": 20000, "deep_depth": 8, "deep_time_ms": 20000 } })
    } else {
        json!({ "version": 1, "engine": { "binary": dir.join("no-engine").display().to_string() } })
    };
    // 不在后台补充深入分析：准备的数据不会在之后改变
    cfg["analysis"]["deepen_unsettled"] = json!(false);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(dir.join("config/config.json"), cfg.to_string()).unwrap();
}

fn start(dir: &Path) -> AppService { AppService::start(paths(dir), Arc::new(Quiet), Arc::new(SimBackend::new())).unwrap() }

fn until(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !f() {
        assert!(Instant::now() < deadline, "等待超时：{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

const RECORD: &str = "1. 炮二平五 马8进7 2. 马二进三 车9平8 3. 车一平二 马2进3";
const ENDGAME: &str = "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w";
/// 收件箱导入的棋局（与准备的对局不同，不会被认作重复）。
const INBOX_RECORD: &str = "1. 兵七进一 卒7进1 2. 马八进七 马8进7";

/// 准备好的数据：一盘导入的对局（一张卡片）、一盘执红的残局（先想后讲没有选对，可以出一道找威胁），
/// 当天的备份，以及导出的学习档案。
struct Prepared {
    game: String,
    game_move: u32,
    card: String,
    endgame: String,
}

fn prepare(dir: &Path) -> Prepared {
    configure(dir, true);
    let service = start(dir);
    until("引擎就绪", || matches!(service.status().engine, EngineState::Ready { .. }));
    let game = service.import_text(RECORD, Some("崩溃测试".into())).unwrap().study.study.id;
    until("讲解完成", || service.study_view(&game).unwrap().stats.pending == 0);
    let view = service.study_view(&game).unwrap();
    let game_move = view.study.nodes[&ROOT].children[0];
    let card = service.add_review(NodeRef { study_id: game.clone(), node_id: game_move }).unwrap().card.id;
    let kind = CreateKind::Fen { fen: ENDGAME.into(), side: None };
    let endgame = service.create_study(CreateStudyRequest { name: None, kind, orientation: None, user_side: Some(Color::Red) }).unwrap().study.study.id;
    let endgame_move =
        service.apply_move(MoveRequest { study_id: endgame.clone(), parent: ROOT, mv: mv("e2c3"), variation: false }).unwrap().node_id.unwrap();
    until("残局讲解完成", || service.study_view(&endgame).unwrap().stats.pending == 0);
    let practice = PracticeRequest {
        study_id: endgame.clone(),
        node_id: endgame_move,
        hints_used: 0,
        revealed_directly: false,
        first_choice: Some(mv("e2g3")),
        hints: None,
    };
    service.record_practice(practice).unwrap();
    service.backup_now().unwrap();
    std::fs::write(dir.join("archive.json"), service.export_archive().unwrap()).unwrap();
    assert!(service.wait_jobs_idle(Duration::from_secs(30)));
    service.shutdown();
    drop(service);
    configure(dir, false);
    Prepared { game, game_move, card, endgame }
}

/// 数据库中全部的表与记录（直接读取副本，不经过迁移）。
fn tables(db: &Path) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
    let tmp = tempfile::tempdir().unwrap();
    let copy = tmp.path().join("copy.redb");
    std::fs::copy(db, &copy).unwrap();
    let db = Database::create(&copy).unwrap();
    let tx = db.begin_read().unwrap();
    let mut out = std::collections::BTreeMap::new();
    for handle in tx.list_tables().unwrap() {
        let name = redb::TableHandle::name(&handle).to_string();
        let t = tx.open_table(TableDefinition::<&str, &str>::new(&name)).unwrap();
        let rows = t.iter().unwrap().map(|r| r.unwrap()).map(|(k, v)| (k.value().to_string(), v.value().to_string())).collect();
        out.insert(name, rows);
    }
    out
}

/// 在子进程中执行一项写入，在 `failpoint` 处终止。返回子进程的输出。
fn crash(dir: &Path, scenario: &str, failpoint: &str, args: Value) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_child", "--nocapture", "--test-threads=1"])
        .env("XQ_CRASH_SCENARIO", scenario)
        .env("XQ_CRASH_DIR", dir)
        .env("XQ_CRASH_FAILPOINT", failpoint)
        .env("XQ_CRASH_ARGS", args.to_string())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{scenario}：子进程应在 {failpoint} 处终止，却正常结束了\n{stdout}\n{stderr}");
    assert!(stderr.contains(&format!("failpoint abort: {failpoint}")), "{scenario}：没有在 {failpoint} 处终止\n{stdout}\n{stderr}");
}

/// 子进程：只在带有 `XQ_CRASH_SCENARIO` 时做事。
#[test]
fn crash_child() {
    let Ok(scenario) = std::env::var("XQ_CRASH_SCENARIO") else { return };
    let dir = PathBuf::from(std::env::var("XQ_CRASH_DIR").unwrap());
    let point = std::env::var("XQ_CRASH_FAILPOINT").unwrap();
    let args: Value = serde_json::from_str(&std::env::var("XQ_CRASH_ARGS").unwrap()).unwrap();
    let arm = || failpoints::set(&point, Some(failpoints::Action::Abort));
    if scenario == "migrate" {
        arm();
        let _ = start(&dir);
        return;
    }
    let service = start(&dir);
    arm();
    let s = |k: &str| args[k].as_str().unwrap().to_string();
    let n = |k: &str| args[k].as_u64().unwrap() as u32;
    match scenario.as_str() {
        "delete_node" => drop(service.delete_node(NodeRef { study_id: s("study"), node_id: n("node") })),
        "record_practice" => drop(service.record_practice(PracticeRequest {
            study_id: s("study"),
            node_id: n("node"),
            hints_used: 1,
            revealed_directly: false,
            first_choice: Some(mv(&s("choice"))),
            hints: None,
        })),
        "exercises" => drop(service.generate_exercises(xq_app::exercise::ExerciseRequest { study_id: s("study"), regenerate: false })),
        "attempt" => drop(service.review_attempt(ReviewAttemptRequest {
            id: s("card"),
            choice: Some(mv(&s("choice"))),
            goal: PracticeGoal::Recall,
            intent: None,
            practice: None,
            request: Some("请求-1".into()),
            hints: 0,
            elapsed_ms: Some(1000),
            confidence: None,
            marks: Vec::new(),
            practice_revision: None,
        })),
        "import_archive" => {
            if let Err(e) = service.import_archive(ArchiveSource::File { path: PathBuf::from(s("file")) }) {
                println!("导入失败：{}", e.message);
            }
        }
        "inbox_import" => drop(service.inbox_import(InboxImportRequest {
            games: vec![InboxImportGame { text: INBOX_RECORD.into(), name: Some("收件箱".into()), user_side: Color::Red }],
        })),
        "export" => drop(service.export_archive_to_file(Path::new(&s("file")))),
        "config" => {
            let mut cfg = service.status().config;
            cfg.analysis.quick_depth = 21;
            drop(service.update_config(cfg));
        }
        other => panic!("没有这个场景：{other}"),
    }
    println!("场景 {scenario} 没有在注入点终止");
}

/// 每个注入点崩溃一次：崩溃后数据与崩溃前完全相同；之后照常执行，得到完整的新状态。
fn each_point(points: &[&str], prepare_dir: &Path, scenario: &str, args: Value, after: impl Fn(&Path)) {
    for point in points {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("home");
        copy_dir(prepare_dir, &dir);
        let before = tables(&dir.join("data/xqlink.redb"));
        crash(&dir, scenario, point, args.clone());
        assert!(tables(&dir.join("data/xqlink.redb")) == before, "{scenario}：在 {point} 处终止后，数据与之前完全相同");
        after(&dir);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// 每一条讲解、AI 讲解与追问记录所属的着法都在。
fn assert_no_orphans(service: &AppService) {
    for summary in service.list_studies().unwrap() {
        let view = service.study_view(&summary.id).unwrap();
        for node in view.explain_status.keys() {
            assert!(view.study.nodes.contains_key(node), "会话 {} 留下了已删除的着法 {node} 的讲解", summary.name);
        }
    }
}

#[test]
fn writes_interrupted_by_a_crash_leave_either_the_whole_old_state_or_the_whole_new_one() {
    let base = tempfile::tempdir().unwrap();
    let home = base.path().join("prepared");
    let p = prepare(&home);

    // 删除着法：会话与这一着的讲解一起删除
    let args = json!({ "study": p.game, "node": p.game_move });
    each_point(&["delete_node:after_study", "delete_node:before_commit"], &home, "delete_node", args, |dir| {
        let service = start(dir);
        assert!(service.study_view(&p.game).unwrap().explain_status.contains_key(&p.game_move), "旧状态：着法与讲解都在");
        service.delete_node(NodeRef { study_id: p.game.clone(), node_id: p.game_move }).unwrap();
        let view = service.study_view(&p.game).unwrap();
        assert!(!view.study.nodes.contains_key(&p.game_move) && view.explain_status.is_empty(), "新状态：着法与讲解都删除了");
        assert_no_orphans(&service);
        service.shutdown();
    });

    // 先想后讲：练习记录与学习事件一起保存
    let args = json!({ "study": p.game, "node": p.game_move, "choice": "h0g2" });
    each_point(&["study_event:after_study", "study_event:before_commit"], &home, "record_practice", args, |dir| {
        let service = start(dir);
        let events = service.learning_events().unwrap().len();
        let view = service.study_view(&p.game).unwrap();
        assert!(view.study.nodes[&p.game_move].practice.is_none(), "旧状态：没有练习记录");
        let req = PracticeRequest {
            study_id: p.game.clone(),
            node_id: p.game_move,
            hints_used: 1,
            revealed_directly: false,
            first_choice: Some(mv("h0g2")),
            hints: None,
        };
        service.record_practice(req).unwrap();
        assert!(service.study_view(&p.game).unwrap().study.nodes[&p.game_move].practice.is_some());
        assert_eq!(service.learning_events().unwrap().len(), events + 1, "新状态：练习记录与学习事件都在");
        service.shutdown();
    });

    // 出题：一批题目一起加入
    let args = json!({ "study": p.endgame });
    each_point(&["exercises:half", "exercises:before_commit"], &home, "exercises", args, |dir| {
        let service = start(dir);
        let cards = service.list_reviews().unwrap().len();
        let summary = service.generate_exercises(xq_app::exercise::ExerciseRequest { study_id: p.endgame.clone(), regenerate: false }).unwrap();
        assert!(summary.threat >= 1, "{}", summary.text);
        assert_eq!(service.list_reviews().unwrap().len(), cards + (summary.threat + summary.choice + summary.better) as usize);
        service.shutdown();
    });

    // 复习作答：作答记录与学习事件一起保存
    let args = json!({ "card": p.card, "choice": "h0g2" });
    each_point(&["attempt:before_commit"], &home, "attempt", args, |dir| {
        let service = start(dir);
        let events = service.learning_events().unwrap().len();
        let req = ReviewAttemptRequest {
            id: p.card.clone(),
            choice: Some(mv("h0g2")),
            goal: PracticeGoal::Recall,
            intent: None,
            practice: None,
            request: Some("请求-1".into()),
            hints: 0,
            elapsed_ms: Some(1000),
            confidence: None,
            marks: Vec::new(),
            practice_revision: None,
        };
        let item = service.review_attempt(req).unwrap();
        assert_eq!(item.card.attempts.len(), 1);
        assert_eq!(service.learning_events().unwrap().len(), events + 1);
        service.shutdown();
    });

    // 收件箱导入：棋局与收件箱记录一起加入
    each_point(&["inbox_import:before_commit"], &home, "inbox_import", json!({}), |dir| {
        let service = start(dir);
        assert!(service.inbox_list().unwrap().items.is_empty());
        service
            .inbox_import(InboxImportRequest { games: vec![InboxImportGame { text: INBOX_RECORD.into(), name: None, user_side: Color::Red }] })
            .unwrap();
        assert_eq!(service.inbox_list().unwrap().items.len(), 1);
        service.shutdown();
    });

    // 导入学习档案到空的数据目录：全部加入或什么都不加入
    let empty = base.path().join("empty");
    configure(&empty, false);
    drop(start(&empty));
    let archive = home.join("archive.json");
    let args = json!({ "file": archive.display().to_string() });
    each_point(&["import:before_commit"], &empty, "import_archive", args, |dir| {
        let service = start(dir);
        assert!(service.list_studies().unwrap().is_empty() && service.list_reviews().unwrap().is_empty(), "旧状态：什么都没有导入");
        service.import_archive(ArchiveSource::File { path: archive.clone() }).unwrap();
        assert_eq!(service.list_studies().unwrap().len(), 2);
        service.shutdown();
    });
}

#[test]
fn a_crash_while_writing_a_file_keeps_the_previous_file() {
    let base = tempfile::tempdir().unwrap();
    let dir = base.path().join("home");
    configure(&dir, false);
    let service = start(&dir);
    service.create_study(CreateStudyRequest { name: Some("导出".into()), kind: CreateKind::Standard, orientation: None, user_side: None }).unwrap();
    service.backup_now().unwrap();
    service.shutdown();
    drop(service);
    let config_before = std::fs::read_to_string(dir.join("config/config.json")).unwrap();
    for point in ["write_atomic:half", "write_atomic:before_rename"] {
        // 导出到已有的文件：原来的文件保持原样
        let file = base.path().join("学习 档案.json");
        std::fs::write(&file, "原来的档案").unwrap();
        crash(&dir, "export", point, json!({ "file": file.display().to_string() }));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "原来的档案", "{point}");
        // 保存设置：原来的设置文件保持原样，仍能读取
        crash(&dir, "config", point, json!({}));
        assert_eq!(std::fs::read_to_string(dir.join("config/config.json")).unwrap(), config_before, "{point}");
    }
    let service = start(&dir);
    assert_eq!(service.status().config.analysis.quick_depth, xq_app::config::AppConfig::default().analysis.quick_depth);
    service.shutdown();
}

#[test]
fn a_crash_during_migration_leaves_the_old_database_and_the_next_start_migrates_it() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/history/v0.3.0/home");
    for point in ["migrate:after_backup", "migrate:after_studies", "migrate:before_commit"] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("home");
        copy_dir(&fixture, &dir);
        configure(&dir, false);
        let before = tables(&dir.join("data/xqlink.redb"));
        crash(&dir, "migrate", point, json!({}));
        assert!(tables(&dir.join("data/xqlink.redb")) == before, "在 {point} 处终止后，数据库仍是迁移前的内容");
        let service = start(&dir);
        assert!(service.status().notices.iter().any(|n| n.contains("升级到")), "下次启动时迁移");
        assert_eq!(service.list_studies().unwrap().len(), 2);
        service.shutdown();
        drop(service);
        let store = xq_app::storage::Store::open(&dir.join("data/xqlink.redb")).unwrap();
        assert_eq!(store.migrations().unwrap().len(), 1, "终止的那次没有记为迁移");
    }
}
