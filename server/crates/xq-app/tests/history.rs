//! 历史数据样本（tests/history/，由 scripts/history-fixtures.mjs 用旧版本的代码生成，生成后不再修改）：
//! 当前版本打开旧版本的数据目录时迁移一次、内容不变；旧版本导出的档案、单个会话与自动备份都能导入，内容相同；
//! 迁移后的数据导出再导入仍相同；来自更新版本的数据库拒绝打开且不改动文件；无法使用的记录移到隔离区。
//!
//! “内容相同”按旧版本自己的接口读出的内容（old-api.json）判断：旧版本读出的每个字段，当前版本读出的值都必须相同
//! （当前版本新增的字段不比较）。由当前代码推算、会随规则改进而变化的内容（主题建议、报告文字等）不在比较范围内。

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use redb::Database;
use redb::ReadableTable;
use redb::TableDefinition;
use serde_json::Value;
use serde_json::json;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::archive::ArchiveSource;
use xq_app::capture::SimBackend;
use xq_app::events::EventSink;
use xq_app::storage::SCHEMA_VERSION;
use xq_app::storage::Store;

const META: TableDefinition<&str, &str> = TableDefinition::new("meta");
const STUDIES: TableDefinition<&str, &str> = TableDefinition::new("studies");
const REVIEWS: TableDefinition<&str, &str> = TableDefinition::new("reviews");
const EXPLANATIONS: TableDefinition<&str, &str> = TableDefinition::new("explanations");

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: &str, _payload: Value) {}
}

fn fixture(name: &str) -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/history").join(name) }

fn repo() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap() }

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

/// 一份可以改动的数据目录副本。
struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Home {
    /// 样本的数据目录副本。`engine` 为 false 时引擎指向不存在的程序：启动后不会有后台分析改动讲解记录。
    fn of(name: &str, engine: bool) -> Home {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("home");
        copy_dir(&fixture(name).join("home"), &root);
        if !engine {
            let path = root.join("config/config.json");
            let mut cfg: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            cfg["engine"]["binary"] = json!(root.join("no-engine").display().to_string());
            std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
        }
        Home { _dir: dir, root }
    }

    fn empty() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("home");
        std::fs::create_dir_all(root.join("config")).unwrap();
        let cfg = json!({ "version": 1, "engine": { "binary": root.join("no-engine").display().to_string() } });
        std::fs::write(root.join("config/config.json"), cfg.to_string()).unwrap();
        Home { _dir: dir, root }
    }

    fn db(&self) -> PathBuf { self.root.join("data/xqlink.redb") }

    fn start(&self) -> AppService {
        let paths = AppPaths {
            data_dir: self.root.join("data"),
            config_dir: self.root.join("config"),
            log_dir: self.root.join("logs"),
            resource_dir: repo(),
            legacy_config: None,
        };
        AppService::start(paths, Arc::new(Quiet), Arc::new(SimBackend::default())).unwrap()
    }

    fn schema_backups(&self) -> Vec<PathBuf> {
        let dir = self.root.join("data/backups");
        let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        out.retain(|p| p.extension().is_some_and(|e| e == "redb"));
        out.sort();
        out
    }
}

fn to_value<T: serde::Serialize>(v: T) -> Value { serde_json::to_value(v).unwrap() }

/// 数据库中全部的表与记录（直接读取，不经过迁移）。读取的是副本：redb 打开文件时会改写文件头。
fn tables(db: &Path) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy.redb");
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

/// 用当前版本的接口读出与 old-api.json 相同的几部分。
fn read(service: &AppService) -> Value {
    let mut studies = serde_json::Map::new();
    for s in service.list_studies().unwrap() {
        studies.insert(s.id.clone(), to_value(service.study_view(&s.id).unwrap()));
    }
    let mut reviews = serde_json::Map::new();
    let list = service.list_reviews().unwrap();
    for item in &list {
        let detail = service.review_detail(xq_app::review::ReviewRef { id: item.card.id.clone() }).unwrap();
        reviews.insert(item.card.id.clone(), to_value(detail));
    }
    json!({
        "list_studies": to_value(service.list_studies().unwrap()),
        "studies": studies,
        "list_reviews": to_value(list),
        "reviews": reviews,
        "learning_events": to_value(service.learning_events().unwrap()),
        "current_practice": to_value(service.current_practice().unwrap()),
    })
}

/// 不属于保存的内容的字段：由当前规则推算（随规则改进而变化），或是这次运行的计数（讲解序号、视图序号、事件流编号）。
const DERIVED: &[&str] = &["themes", "explain_seq", "view_seq", "stream_id"];

/// `old` 中的每个字段在 `new` 中都有相同的值（`new` 可以多出字段）。不同时返回第一处不同的位置。
fn contains(old: &Value, new: &Value, path: &str) -> Result<(), String> {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                if DERIVED.contains(&k.as_str()) {
                    continue;
                }
                let Some(w) = b.get(k) else { return Err(format!("{path}.{k}：当前版本没有这个字段")) };
                contains(v, w, &format!("{path}.{k}"))?;
            }
            Ok(())
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Err(format!("{path}：旧版本 {} 项，当前版本 {} 项", a.len(), b.len()));
            }
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                contains(v, w, &format!("{path}[{i}]"))?;
            }
            Ok(())
        }
        (a, b) if a == b => Ok(()),
        (a, b) => Err(format!("{path}：旧版本 {a}，当前版本 {b}")),
    }
}

fn assert_contains(old: &Value, new: &Value, what: &str) {
    if let Err(e) = contains(old, new, "") {
        panic!("{what}与旧版本读出的不同：{e}");
    }
}

/// 旧版本读出的内容中与 `read` 相同的几部分（AI 讲解、整局报告与学习画像由当前规则推算，另外检查）。
/// 两份内容完全相同；不同时指出第一处不同的位置。
fn assert_same(a: &Value, b: &Value, what: &str) {
    if let Err(e) = contains(a, b, "").and_then(|_| contains(b, a, "")) {
        panic!("{what}：{e}");
    }
}

fn old_api(name: &str) -> Value {
    let all: Value = serde_json::from_str(&std::fs::read_to_string(fixture(name).join("old-api.json")).unwrap()).unwrap();
    let mut out = serde_json::Map::new();
    for k in ["list_studies", "studies", "list_reviews", "reviews", "learning_events", "current_practice", "inbox_list"] {
        if let Some(v) = all.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    Value::Object(out)
}

/// 只比较会话、卡片与学习事件（档案中有的部分）。
fn saved_parts(v: &Value) -> Value {
    json!({ "studies": v["studies"], "list_reviews": v["list_reviews"], "learning_events": v["learning_events"] })
}

fn check_fixture(name: &str) {
    let old = old_api(name);
    let home = Home::of(name, false);
    let original = tables(&home.db());

    let service = home.start();
    let notices = service.status().notices;
    assert!(
        notices.iter().any(|n| n.contains(&format!("已从版本 1 升级到 {SCHEMA_VERSION}"))),
        "启动时说明升级了数据：{notices:?}"
    );
    let first = read(&service);
    let mut stored = old.clone();
    stored.as_object_mut().unwrap().remove("inbox_list");
    assert_contains(&stored, &first, "迁移后读出的内容");
    service.shutdown();
    drop(service);

    // 迁移前的数据库副本与原来的数据库内容相同（逐表逐条）；迁移只做一次
    let backups = home.schema_backups();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert!(tables(&backups[0]) == original, "迁移前的副本与原来的数据库内容相同");
    assert_ne!(tables(&home.db()), original, "迁移改写了记录");
    {
        let store = Store::open(&home.db()).unwrap();
        assert!(store.migration().is_none(), "再次打开时不再迁移");
        let migrations = store.migrations().unwrap();
        assert_eq!(migrations.len(), 1);
        assert_eq!((migrations[0].from, migrations[0].to), (1, SCHEMA_VERSION));
        assert_eq!(migrations[0].quarantined, 0, "样本中没有无法使用的记录");
        assert!(store.quarantined().unwrap().is_empty());
    }
    let service = home.start();
    assert!(!service.status().notices.iter().any(|n| n.contains("升级到")), "第二次启动不再说明升级");
    assert_same(&first, &read(&service), "重新启动后读出的内容不变");
    assert_eq!(home.schema_backups().len(), 1, "第二次启动不再备份数据库");

    // 迁移后导出的档案导入到空的数据目录：内容相同
    let exported = service.export_archive().unwrap();
    service.shutdown();
    let fresh = Home::empty();
    let other = fresh.start();
    other.import_archive(ArchiveSource::Text { text: exported }).unwrap();
    assert_contains(&saved_parts(&old), &read(&other), "导出再导入的内容");
    other.shutdown();

    // 旧版本导出的档案与当天的自动备份：导入到空的数据目录，内容相同
    let backup = std::fs::read_dir(fixture(name).join("home/data/backups"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("xqlink-archive-"))
        .expect("样本中有自动备份");
    for file in [fixture(name).join("archive.json"), backup] {
        let fresh = Home::empty();
        let other = fresh.start();
        other.import_archive(ArchiveSource::File { path: file.clone() }).unwrap();
        assert_contains(&saved_parts(&old), &read(&other), &format!("导入 {} 的内容", file.display()));
        other.shutdown();
    }

    // 旧版本导出的单个会话：换一个编号导入，棋谱树与讲解相同
    let text = std::fs::read_to_string(fixture(name).join("study-export.json")).unwrap();
    let source: Value = serde_json::from_str(&text).unwrap();
    let fresh = Home::empty();
    let other = fresh.start();
    let imported = other.import_text(&text, None).unwrap();
    let view = to_value(other.study_view(&imported.study.study.id).unwrap());
    assert_ne!(view["study"]["id"], source["study"]["id"], "单个会话用新的编号导入");
    assert_contains(&source["study"]["nodes"], &view["study"]["nodes"], "导入的单个会话的棋谱树");
    let old_view = &old["studies"][source["study"]["id"].as_str().unwrap()];
    assert_contains(&old_view["explain_status"], &view["explain_status"], "导入的单个会话的讲解状态");
    other.shutdown();
}

#[test]
fn the_v0_3_0_release_data_migrates_once_and_keeps_its_content() { check_fixture("v0.3.0"); }

#[test]
fn the_0_4_inbox_data_migrates_once_and_keeps_its_content() {
    check_fixture("ab20742-inbox");
    // 收件箱：棋局、我执哪一方与分析进度不变
    let old = old_api("ab20742-inbox");
    let home = Home::of("ab20742-inbox", false);
    let service = home.start();
    let list = to_value(service.inbox_list().unwrap());
    let mut items = old["inbox_list"]["items"].as_array().unwrap().clone();
    let mut now = list["items"].as_array().unwrap().clone();
    for v in [&mut items, &mut now] {
        v.sort_by_key(|i| i["study_id"].as_str().unwrap().to_string());
    }
    assert_contains(&Value::Array(items), &Value::Array(now), "收件箱");
    service.shutdown();
}

#[test]
fn the_v0_3_0_config_and_key_still_load() {
    let home = Home::of("v0.3.0", true);
    let service = home.start();
    let status = service.status();
    assert!(status.notices.iter().all(|n| !n.contains("配置")), "旧版本的配置照常读取，不需要备份或改正：{:?}", status.notices);
    assert!(status.coach.key_configured, "旧版本保存的密钥仍然可用");
    assert!(status.config.coach.enabled);
    assert_eq!(status.config.analysis.quick_depth, 8);
    service.shutdown();
}

#[test]
fn work_interrupted_by_a_killed_v0_3_0_process_is_settled_at_startup() {
    // 样本在 AI 讲解与追问生成中、两着讲解进行中、一次局面比较等待引擎时结束了进程
    let home = Home::of("v0.3.0-interrupted", false);
    let service = home.start();
    let decisions: Vec<Value> = service
        .list_reviews()
        .unwrap()
        .iter()
        .flat_map(|item| item.card.attempts.iter().filter_map(|a| a.decision.as_ref()).map(to_value).collect::<Vec<_>>())
        .collect();
    assert!(decisions.iter().any(|d| d["state"] == "failed" && d["error"].as_str().unwrap_or("").contains("没有完成")), "{decisions:?}");
    assert!(decisions.iter().all(|d| d["state"] != "pending"), "没有一直等待的局面比较：{decisions:?}");
    let mut coach_states = Vec::new();
    for s in service.list_studies().unwrap() {
        let view = to_value(service.study_view(&s.id).unwrap());
        for (node, _) in view["study"]["nodes"].as_object().unwrap() {
            let r = xq_app::api::NodeRef { study_id: s.id.clone(), node_id: node.parse().unwrap() };
            coach_states.push(to_value(service.coach_view(r).unwrap())["status"].clone());
        }
        // 打开会话时，上次没有完成的讲解重新排队；引擎不可用时如实标为失败，不会一直显示进行中
        service.load_study(&s.id).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while service.study_view(&s.id).unwrap().stats.pending > 0 {
            assert!(std::time::Instant::now() < deadline, "会话 {} 的讲解一直进行中", s.name);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    assert!(coach_states.iter().all(|s| s != "queued" && s != "running"), "AI 讲解没有一直排队或生成中：{coach_states:?}");
    assert!(coach_states.iter().any(|s| s == "failed" || s == "none" || s == "stale"), "{coach_states:?}");
    let practice = service.current_practice().unwrap().expect("没有做完的一组练习仍在");
    assert_eq!(practice.index, 1, "练习停在原来的题目");
    service.shutdown();
}

/// 直接改写数据库副本中的记录（模拟损坏或较新版本的数据）。
fn poke(db: &Path, f: impl FnOnce(&redb::WriteTransaction)) {
    let db = Database::create(db).unwrap();
    let tx = db.begin_write().unwrap();
    f(&tx);
    tx.commit().unwrap();
}

#[test]
fn a_database_from_a_newer_version_is_refused_and_left_untouched() {
    let home = Home::of("v0.3.0", false);
    poke(&home.db(), |tx| {
        tx.open_table(META).unwrap().insert("schema_version", "99").unwrap();
    });
    let before = std::fs::read(home.db()).unwrap();
    let err = Store::open(&home.db()).err().expect("拒绝打开");
    assert!(err.0.contains("更新的版本（99）"), "{}", err.0);
    assert!(std::fs::read(home.db()).unwrap() == before, "文件没有改动");
    assert!(home.schema_backups().is_empty(), "没有迁移，也就没有备份");

    poke(&home.db(), |tx| {
        tx.open_table(META).unwrap().insert("schema_version", "版本二").unwrap();
    });
    let err = Store::open(&home.db()).err().expect("无法识别的版本也拒绝打开");
    assert!(err.0.contains("无法识别"), "{}", err.0);
}

#[test]
fn unusable_records_are_kept_aside_and_orphans_removed_during_migration() {
    let old = old_api("v0.3.0");
    let home = Home::of("v0.3.0", false);
    let study_id = old["list_studies"][0]["id"].as_str().unwrap().to_string();
    poke(&home.db(), |tx| {
        let mut reviews = tx.open_table(REVIEWS).unwrap();
        reviews.insert("坏的卡片", "{ 不是 JSON").unwrap();
        drop(reviews);
        // 编号不小于 next_id 的节点：之后的走子会覆盖它，会话不成立
        let mut studies = tx.open_table(STUDIES).unwrap();
        let raw = studies.get(study_id.as_str()).unwrap().unwrap().value().to_string();
        let mut broken: Value = serde_json::from_str(&raw).unwrap();
        broken["id"] = json!("broken-study");
        broken["next_id"] = json!(1);
        studies.insert("broken-study", broken.to_string().as_str()).unwrap();
        drop(studies);
        // 已经删除的着法留下的讲解记录
        let mut explanations = tx.open_table(EXPLANATIONS).unwrap();
        let first = explanations.iter().unwrap().next().unwrap().unwrap().1.value().to_string();
        let mut orphan: Value = serde_json::from_str(&first).unwrap();
        orphan["node_id"] = json!(999);
        let key = format!("{}/{:010}", orphan["study_id"].as_str().unwrap(), 999);
        explanations.insert(key.as_str(), orphan.to_string().as_str()).unwrap();
    });

    let service = home.start();
    let notices = service.status().notices;
    assert!(notices.iter().any(|n| n.contains("2 条无法读取的记录已原样保留在隔离区")), "{notices:?}");
    assert_contains(&saved_parts(&old), &read(&service), "其余内容");
    service.shutdown();
    drop(service);

    let store = Store::open(&home.db()).unwrap();
    let mut kept = store.quarantined().unwrap();
    kept.sort_by(|a, b| a.table.cmp(&b.table));
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!((kept[0].table.as_str(), kept[0].key.as_str()), ("reviews", "坏的卡片"));
    assert_eq!(kept[0].raw, "{ 不是 JSON", "原样保存");
    assert_eq!((kept[1].table.as_str(), kept[1].key.as_str()), ("studies", "broken-study"));
    assert!(kept[1].reason.contains("编号"), "{}", kept[1].reason);
    let entry = &store.migrations().unwrap()[0];
    assert_eq!((entry.quarantined, entry.removed), (2, 1), "{entry:?}");
}

#[test]
#[ignore = "手动查看样本数据库的内容"]
fn print_fixture_tables() {
    let name = std::env::var("FIXTURE").unwrap_or_else(|_| "v0.3.0-interrupted".into());
    for (table, rows) in tables(&fixture(&name).join("home/data/xqlink.redb")) {
        for (k, v) in rows {
            println!("{table}\t{k}\t{v}");
        }
    }
}

/// 0.1.x 的配置文件（按 0.1.2 的 `Config::save` 的格式：serde_json 缩进输出）。
#[test]
fn v0_1_2_config_files_are_carried_over_and_left_in_place() {
    for (name, deep_ms, threads, hash, cloud, cloud_ms, depth, interval) in
        [("default", 5000, 4, 64, true, 5000, 20, 100), ("time-bug", 8000, 8, 256, false, 3000, 25, 150)]
    {
        let home = Home::empty();
        std::fs::remove_file(home.root.join("config/config.json")).unwrap();
        let legacy = fixture("v0.1.2-config").join(name).join("xqlink/config.json");
        let before = std::fs::read(&legacy).unwrap();
        let paths = AppPaths {
            data_dir: home.root.join("data"),
            config_dir: home.root.join("config"),
            log_dir: home.root.join("logs"),
            resource_dir: home.root.join("no-resources"),
            legacy_config: Some(legacy.clone()),
        };
        let service = AppService::start(paths, Arc::new(Quiet), Arc::new(SimBackend::default())).unwrap();
        let status = service.status();
        assert!(status.notices.iter().any(|n| n.contains("已从旧版本配置迁移设置")), "{name}：{:?}", status.notices);
        let cfg = &status.config;
        assert_eq!(
            (cfg.analysis.deep_time_ms, cfg.engine.threads, cfg.engine.hash_mb, cfg.cloud.enabled, cfg.cloud.timeout_ms),
            (deep_ms, threads, hash, cloud, cloud_ms),
            "{name}"
        );
        assert_eq!((cfg.analysis.deep_depth, cfg.capture.interval_ms), (depth, interval), "{name}");
        if name == "time-bug" {
            assert!(status.notices.iter().any(|n| n.contains("重复换算单位")), "{:?}", status.notices);
        }
        assert!(home.root.join("config/config.json").exists(), "设置写到新的位置");
        assert!(std::fs::read(&legacy).unwrap() == before, "旧版本的配置文件保持原样");
        service.shutdown();
    }
}

#[test]
fn archives_and_study_files_from_a_newer_version_are_refused() {
    let home = Home::empty();
    let service = home.start();
    let mut archive: Value = serde_json::from_str(&std::fs::read_to_string(fixture("v0.3.0").join("archive.json")).unwrap()).unwrap();
    archive["version"] = json!(99);
    let Err(err) = service.preview_archive(ArchiveSource::Text { text: archive.to_string() }) else { panic!("拒绝预览") };
    assert!(err.message.contains("更新的版本"), "{}", err.message);
    let Err(err) = service.import_archive(ArchiveSource::Text { text: archive.to_string() }) else { panic!("拒绝导入") };
    assert!(err.message.contains("更新的版本"), "{}", err.message);
    let mut study: Value = serde_json::from_str(&std::fs::read_to_string(fixture("v0.3.0").join("study-export.json")).unwrap()).unwrap();
    study["version"] = json!(99);
    let Err(err) = service.import_text(&study.to_string(), None) else { panic!("拒绝导入单个会话") };
    assert!(err.message.contains("更新的版本"), "{}", err.message);
    assert!(service.list_studies().unwrap().is_empty() && service.list_reviews().unwrap().is_empty(), "没有导入任何内容");
    service.shutdown();
}

/// 样本生成后不再修改：每个文件的校验值记在 tests/history/SHA256SUMS 中，也不能多出或少了文件。
#[test]
fn historical_fixtures_are_never_modified() {
    use sha2::Digest;
    let root = fixture("");
    let manifest = std::fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    let mut listed = std::collections::BTreeMap::new();
    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        let (hash, file) = line.split_once("  ").expect("每行是“校验值  路径”");
        listed.insert(file.to_string(), hash.to_string());
    }
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.retain(|f| f != "SHA256SUMS" && f != "README.md");
    files.sort();
    assert_eq!(files, listed.keys().cloned().collect::<Vec<_>>(), "样本中的文件与 SHA256SUMS 列出的不同");
    for (file, hash) in &listed {
        let digest = sha2::Sha256::digest(std::fs::read(root.join(file)).unwrap());
        let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(&actual, hash, "{file} 被修改过：样本生成后不再修改，需要新的样本时另建一个目录");
    }
}
