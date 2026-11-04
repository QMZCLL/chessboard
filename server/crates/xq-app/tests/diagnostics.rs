//! 诊断包（见 `support`）：包含定位故障所需的内容，不包含密钥、接口地址中的凭据、本机路径；
//! 默认也不包含棋谱内容，选择包含时附上学习档案。

use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use serde_json::json;
use xq_app::AppPaths;
use xq_app::AppService;
use xq_app::api::*;
use xq_app::capture::SimBackend;
use xq_app::events::EventSink;
use xq_app::support::DiagnosticsRequest;
use xq_app::support::zip;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: &str, _payload: Value) {}
}

const SECRET: &str = "sk-test-very-secret-0123456789";
const STUDY: &str = "我的秘密对局";
const PLAYER: &str = "王小明";

fn start(dir: &Path) -> AppService {
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let cfg = json!({
        "version": 1,
        "engine": { "binary": dir.join("no-engine").display().to_string() },
        "coach": { "enabled": true, "provider": "deepseek", "base_url": "https://bob:hunter2@llm.example.com/v1?api_key=leak", "model": "m" },
        "player": { "names": [PLAYER] },
    });
    std::fs::write(dir.join("config/config.json"), cfg.to_string()).unwrap();
    let paths = AppPaths {
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        log_dir: dir.join("logs"),
        resource_dir: dir.join("resources"),
        legacy_config: None,
    };
    AppService::start(paths, Arc::new(Quiet), Arc::new(SimBackend::new())).unwrap()
}

fn files(bytes: &[u8]) -> std::collections::BTreeMap<String, String> {
    zip::read(bytes).into_iter().map(|(n, b)| (n, String::from_utf8(b).unwrap())).collect()
}

#[test]
fn the_bundle_has_what_support_needs_and_none_of_the_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("用户 目录");
    let service = start(&dir);
    service.set_coach_credential(CoachCredentialRequest { key: Some(SECRET.into()) }).unwrap();
    let study = service
        .import_text("1. 炮二平五 马8进7 2. 马二进三 车9平8", Some(STUDY.into()))
        .unwrap()
        .study
        .study
        .id;
    // 日志中可能出现的内容：密钥、路径、局面与着法
    std::fs::create_dir_all(dir.join("logs")).unwrap();
    let log = format!(
        "{{\"level\":\"WARN\",\"message\":\"请求失败 Authorization: Bearer {SECRET} 数据目录 {} 局面 rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w 着法 h2e2 炮二平五 会话 {STUDY}\"}}\n",
        dir.join("data").display()
    );
    std::fs::write(dir.join("logs/runtime.2026-11-03.log"), log).unwrap();

    let bundle = files(&service.diagnostic_bundle(DiagnosticsRequest { include_games: false }).unwrap());
    for name in ["manifest.json", "system-summary.json", "configuration-redacted.json", "schema.json", "job-history.json", "logs/runtime.2026-11-03.log"] {
        assert!(bundle.contains_key(name), "缺少 {name}：{:?}", bundle.keys().collect::<Vec<_>>());
    }
    assert!(!bundle.contains_key("archive.json"), "默认不含学习档案");
    let manifest: Value = serde_json::from_str(&bundle["manifest.json"]).unwrap();
    assert_eq!(manifest["include_games"], false);
    assert_eq!(manifest["files"].as_array().unwrap().len(), bundle.len() - 1);
    let schema: Value = serde_json::from_str(&bundle["schema.json"]).unwrap();
    assert_eq!(schema["schema_version"], xq_app::storage::SCHEMA_VERSION);
    let jobs: Value = serde_json::from_str(&bundle["job-history.json"]).unwrap();
    assert!(jobs["finished"].as_array().unwrap().iter().any(|j| j["kind"] == "import_study" && j["state"] == "succeeded"), "{jobs}");
    let summary: Value = serde_json::from_str(&bundle["system-summary.json"]).unwrap_or_else(|e| panic!("{e}\n{}", bundle["system-summary.json"]));
    assert_eq!((summary["data"]["studies"].as_u64(), summary["coach"]["key_configured"].as_bool()), (Some(1), Some(true)));
    let config: Value = serde_json::from_str(&bundle["configuration-redacted.json"]).unwrap_or_else(|e| panic!("{e}\n{}", bundle["configuration-redacted.json"]));
    assert_eq!(config["coach"]["credential_ref"], "store:coach", "只有密钥的引用");

    let everything: String = bundle.values().cloned().collect();
    let home = dir.display().to_string();
    for gone in [SECRET, "hunter2", "bob:", "api_key=leak", &home, STUDY, PLAYER, "rnbakabnr", "炮二平五", "h2e2", "马8进7"] {
        assert!(!everything.contains(gone), "诊断包中出现了 {gone}");
    }
    assert!(everything.contains("llm.example.com/v1"), "保留接口的主机，便于判断连接问题");
    assert!(bundle["logs/runtime.2026-11-03.log"].contains("<数据目录>"), "{}", bundle["logs/runtime.2026-11-03.log"]);

    // 选择包含棋谱内容：附上学习档案，会话名称与着法保留，密钥仍然清除
    let full = files(&service.diagnostic_bundle(DiagnosticsRequest { include_games: true }).unwrap());
    assert!(full["archive.json"].contains(STUDY) && full["archive.json"].contains(&study));
    let everything: String = full.values().cloned().collect();
    assert!(!everything.contains(SECRET) && !everything.contains("hunter2") && !everything.contains(&home));

    // 导出到文件：写成完整的 zip，并登记为一次导出
    let out = dir.join("诊断 包.zip");
    service.export_diagnostics_to_file(&out, DiagnosticsRequest { include_games: false }).unwrap();
    assert!(files(&std::fs::read(&out).unwrap()).contains_key("manifest.json"));
    assert!(service.jobs().finished.iter().any(|j| j.key == "export_diagnostics" && j.committed));
    service.shutdown();
}
