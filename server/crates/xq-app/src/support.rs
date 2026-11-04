//! 诊断包：用户反馈问题时导出的一个 zip 文件，用来定位故障，不需要另外描述环境。
//!
//! 内容：
//! - `manifest.json`：格式、应用版本、生成时间、是否含棋谱内容、文件列表与清除规则；
//! - `system-summary.json`：系统、引擎状态、识别、AI 讲解是否可用、最近的提示与数据文件的大小；
//! - `configuration-redacted.json`：清除了敏感信息的设置；
//! - `schema.json`：数据版本、做过的迁移、隔离区中的记录（只有表、键与原因）；
//! - `job-history.json`：后台任务的登记（见 `jobs`）；
//! - `logs/`：最近的日志（每份最多 1 MB，合计最多 2 MB）；
//! - 只有选择“包含棋谱内容”时：`archive.json`（学习档案）与隔离区记录的原文。
//!
//! 写入包中的每一份文字都经过 `Redactor`：清除密钥（包括本机保存的密钥原文）、Authorization 头、
//! 接口地址中的用户名密码与查询参数、本机用户名与完整路径；不含棋谱内容时还清除 FEN、着法与会话名称。

use std::path::Path;
use std::path::PathBuf;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::jobs::JobKind;
use crate::service::APP_VERSION;
use crate::service::ApiResult;
use crate::service::AppService;
use crate::service::now_ms;

const FORMAT: &str = "xqlink-diagnostics";
const FORMAT_VERSION: u32 = 1;
const LOG_FILE_LIMIT: usize = 1024 * 1024;
const LOG_TOTAL_LIMIT: usize = 2 * 1024 * 1024;
const LOG_FILES: usize = 3;

/// 导出诊断包的请求。
#[derive(Clone, Debug, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct DiagnosticsRequest {
    /// 附上棋谱内容（学习档案与隔离区记录的原文）；默认不含，也会清除日志中的局面、着法与会话名称。
    #[serde(default)]
    pub include_games: bool,
}

/// 清除规则的说明（写在 manifest.json 中）。
const RULES: &[&str] = &[
    "密钥：本机保存的密钥原文、sk- 开头的密钥、Bearer 令牌、api_key/token/secret/password 字段的值",
    "接口地址：用户名、密码与查询参数",
    "路径：数据、配置、日志与资源目录，用户主目录；本机用户名",
    "不含棋谱内容时：FEN 局面、坐标着法、中文记谱、会话名称与“我的名字”",
];

/// 清除诊断包文字中的敏感信息。
pub struct Redactor {
    /// 原样替换的文字（较长的在前，避免较短的先替换了一部分）。
    exact: Vec<(String, String)>,
    patterns: Vec<(Regex, &'static str)>,
    games: Vec<(Regex, &'static str)>,
    include_games: bool,
}

impl Redactor {
    /// `secrets` 是需要原样清除的文字（如本机保存的密钥）；`paths` 是本机目录与它们的替代名称；
    /// `names` 是会话名称与“我的名字”（不含棋谱内容时清除）。
    pub fn new(secrets: &[String], paths: &[(String, &str)], names: &[String], include_games: bool) -> Self {
        let mut exact: Vec<(String, String)> = Vec::new();
        for s in secrets.iter().filter(|s| s.len() >= 4) {
            exact.push((s.clone(), "<已清除的密钥>".into()));
        }
        for (p, label) in paths.iter().filter(|(p, _)| p.len() >= 3) {
            exact.push((p.clone(), label.to_string()));
            // JSON 中的反斜杠是转义过的（Windows 路径）
            if p.contains('\\') {
                exact.push((p.replace('\\', "\\\\"), label.to_string()));
            }
        }
        if !include_games {
            for n in names.iter().filter(|n| n.chars().count() >= 2) {
                exact.push((n.clone(), "<会话名称>".into()));
            }
        }
        if let Some(user) = ["USERNAME", "USER"].iter().find_map(|k| std::env::var(k).ok()).filter(|u| u.len() >= 3) {
            exact.push((user, "<用户名>".into()));
        }
        exact.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
        let re = |s: &str| Regex::new(s).expect("诊断包的清除规则");
        let patterns = vec![
            (re(r"sk-[A-Za-z0-9_\-]{6,}"), "sk-<已清除>"),
            (re(r"(?i)bearer\s+[A-Za-z0-9._~+/\-]{6,}=*"), "Bearer <已清除>"),
            (re(r#"(?i)("?(?:api[_-]?key|token|secret|password|authorization)"?\s*[:=]\s*"?)[^"\s,}&]{3,}"#), "${1}<已清除>"),
            // 接口地址中的用户名与密码、查询参数
            // （不跨过引号与 JSON 的分隔符：清除后仍是合法的 JSON）
            (re(r#"(?i)(https?://)[^/\s@:"']+(?::[^/\s@"']*)?@"#), "${1}<已清除>@"),
            (re(r#"(?i)(https?://[^\s?#"']+)\?[^\s#"',}\]]+"#), "${1}?<已清除>"),
        ];
        let games = vec![
            // FEN：十行棋盘与行棋方
            (re(r"[rnbakcpRNBAKCP1-9]{1,9}(?:/[rnbakcpRNBAKCP1-9]{1,9}){9}(?:\s+[wbr])?(?:\s+-\s+-\s+\d+\s+\d+)?"), "<局面>"),
            // 中文记谱，如“炮二平五”“马8进7”“前车进一”
            (re(r"[前后中一二三四五]?[车車马馬炮砲相象仕士帅帥将將兵卒][一二三四五六七八九1-9１-９]?[进進退平][一二三四五六七八九1-9１-９]"), "<着法>"),
            // 坐标着法，如 h2e2：前后都不是字母、数字、“-”或“_”（不会改动编号与校验值中的片段）
            (re(r"(^|[^0-9A-Za-z_\-])[a-i][0-9][a-i][0-9]([^0-9A-Za-z_\-]|$)"), "${1}<着法>${2}"),
        ];
        Self { exact, patterns, games, include_games }
    }

    pub fn text(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (from, to) in &self.exact {
            if out.contains(from.as_str()) {
                out = out.replace(from.as_str(), to);
            }
        }
        for (re, to) in &self.patterns {
            out = re.replace_all(&out, *to).into_owned();
        }
        if !self.include_games {
            for (re, to) in &self.games {
                // 相邻的两步（如“h2e2 h9g7”）共用中间的分隔符，一次替换只换得到其中一步：换到不再变化为止
                for _ in 0..4 {
                    let next = re.replace_all(&out, *to).into_owned();
                    if next == out {
                        break;
                    }
                    out = next;
                }
            }
        }
        out
    }

    pub fn json(&self, v: &Value) -> String { self.text(&serde_json::to_string_pretty(v).unwrap_or_default()) }
}

/// 最近的几份日志（按文件名，较新的在前），每份取末尾，合计不超过上限。
fn recent_logs(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "log")).collect())
        .unwrap_or_default();
    files.sort();
    files.reverse();
    let mut total = 0;
    let mut out = Vec::new();
    for path in files.into_iter().take(LOG_FILES) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let limit = LOG_FILE_LIMIT.min(LOG_TOTAL_LIMIT - total);
        if limit == 0 {
            break;
        }
        let start = bytes.len().saturating_sub(limit);
        // 从截取处的下一行开始，不留下半行（也不会切开一个字的 UTF-8 编码）
        let start = if start > 0 { bytes[start..].iter().position(|b| *b == b'\n').map_or(bytes.len(), |i| start + i + 1) } else { 0 };
        total += bytes.len() - start;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        out.push((name, bytes[start..].to_vec()));
    }
    out
}

fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|it| it.filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

/// 数据版本、做过的迁移与隔离区中的记录数（安装包的冒烟测试与诊断用）。
#[derive(Clone, Debug, serde::Serialize)]
pub struct SchemaStatus {
    pub schema_version: u32,
    pub migrations: Vec<crate::storage::MigrationEntry>,
    pub quarantined: usize,
}

impl AppService {
    pub fn schema_status(&self) -> SchemaStatus {
        SchemaStatus {
            schema_version: crate::storage::SCHEMA_VERSION,
            migrations: self.inner.store.migrations().unwrap_or_default(),
            quarantined: self.inner.store.quarantined().map(|q| q.len()).unwrap_or(0),
        }
    }

    /// 生成诊断包（zip 文件的内容）。
    pub fn diagnostic_bundle(&self, req: DiagnosticsRequest) -> ApiResult<Vec<u8>> {
        let include = req.include_games;
        let inner = &self.inner;
        let status = self.status();
        let paths = &inner.paths;
        let cfg = inner.config_snapshot();
        let secrets: Vec<String> =
            crate::llm::credentials::resolve(&paths.config_dir, cfg.coach.credential_ref.as_deref()).ok().flatten().into_iter().collect();
        let home = ["HOME", "USERPROFILE"].iter().find_map(|k| std::env::var(k).ok()).unwrap_or_default();
        let shown = |p: &Path| p.display().to_string();
        let mut dirs = vec![
            (shown(&paths.data_dir), "<数据目录>"),
            (shown(&paths.config_dir), "<配置目录>"),
            (shown(&paths.log_dir), "<日志目录>"),
            (shown(&paths.resource_dir), "<程序目录>"),
        ];
        // 自选的引擎、网络文件与识别模型所在的目录（设置与出错信息中会出现）
        for file in [cfg.engine.binary.as_ref(), cfg.engine.nnue.as_ref(), cfg.capture.model.as_ref()].into_iter().flatten() {
            if let Some(parent) = file.parent() {
                dirs.push((shown(parent), "<引擎目录>"));
            }
        }
        dirs.push((home, "~"));
        let mut names: Vec<String> = inner.store.list_studies().map(|s| s.into_iter().map(|s| s.name).collect()).unwrap_or_default();
        names.extend(cfg.player.names.iter().cloned());
        let redact = Redactor::new(&secrets, &dirs, &names, include);

        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let engine = serde_json::to_value(&status.engine).unwrap_or(Value::Null);
        let db = paths.data_dir.join("xqlink.redb");
        let summary = json!({
            "app_version": APP_VERSION,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "engine": engine,
            "recognition": { "available": status.recognition.available, "model": status.recognition.model, "detail": status.recognition.detail },
            "capture": serde_json::to_value(&status.capture).ok().and_then(|v| v.get("state").cloned()),
            "coach": {
                "enabled": status.coach.enabled,
                "ready": status.coach.ready,
                "key_configured": status.coach.key_configured,
                "model_label": status.coach.model_label,
                "used_today": status.coach.used_today,
                "daily_budget": status.coach.daily_budget,
                "reason": status.coach.reason,
            },
            "notices": status.notices,
            "data": {
                "database_bytes": std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0),
                "backups_bytes": dir_size(&paths.data_dir.join("backups")),
                "studies": inner.store.list_studies().map(|s| s.len()).unwrap_or(0),
                "reviews": inner.store.list_reviews().map(|s| s.len()).unwrap_or(0),
                "learning_events": inner.store.list_events().map(|s| s.len()).unwrap_or(0),
            },
        });
        files.push(("system-summary.json".into(), redact.json(&summary).into_bytes()));

        let mut config = serde_json::to_value(&cfg).unwrap_or(Value::Null);
        if !include && let Some(names) = config.pointer_mut("/player/names") {
            *names = json!(format!("<{} 个名字>", cfg.player.names.len()));
        }
        files.push(("configuration-redacted.json".into(), redact.json(&config).into_bytes()));

        let quarantined = inner.store.quarantined().unwrap_or_default();
        let schema = json!({
            "schema_version": crate::storage::SCHEMA_VERSION,
            "migrations": inner.store.migrations().unwrap_or_default(),
            "quarantined": quarantined.iter().map(|q| json!({ "table": q.table, "key": q.key, "reason": q.reason, "at": q.at })).collect::<Vec<_>>(),
        });
        files.push(("schema.json".into(), redact.json(&schema).into_bytes()));
        files.push(("job-history.json".into(), redact.json(&serde_json::to_value(self.jobs()).unwrap_or(Value::Null)).into_bytes()));
        let logs = recent_logs(&paths.log_dir);
        if logs.is_empty() {
            files.push(("logs/README.txt".into(), "没有找到日志文件（开发桥只把日志写到控制台）。\n".as_bytes().to_vec()));
        }
        for (name, bytes) in logs {
            files.push((format!("logs/{name}"), redact.text(&String::from_utf8_lossy(&bytes)).into_bytes()));
        }
        if include {
            files.push(("archive.json".into(), redact.text(&self.export_archive()?).into_bytes()));
            let raw: Vec<Value> = quarantined.iter().map(|q| json!({ "table": q.table, "key": q.key, "raw": q.raw })).collect();
            files.push(("quarantine-raw.json".into(), redact.json(&Value::Array(raw)).into_bytes()));
        }
        let manifest = json!({
            "format": FORMAT,
            "version": FORMAT_VERSION,
            "app_version": APP_VERSION,
            "created_at": now_ms(),
            "include_games": include,
            "files": files.iter().map(|(n, b)| json!({ "name": n, "bytes": b.len() })).collect::<Vec<_>>(),
            "redaction": RULES,
        });
        files.insert(0, ("manifest.json".into(), serde_json::to_string_pretty(&manifest).unwrap_or_default().into_bytes()));
        Ok(zip::store(&files))
    }

    /// 导出诊断包到文件（先写临时文件再替换）。
    pub fn export_diagnostics_to_file(&self, path: &Path, req: DiagnosticsRequest) -> ApiResult<()> {
        self.inner.tracked(JobKind::Export, "export_diagnostics", None, || {
            let bytes = self.diagnostic_bundle(req)?;
            crate::files::write_atomic(path, &bytes).map_err(|e| format!("写入 {} 失败：{e}", path.display()).into())
        })
    }
}

/// 最简单的 zip：每个文件按原样存储（不压缩），文件名用 UTF-8。
pub mod zip {
    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for b in data {
            crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        !crc
    }

    /// 把文件写成 zip（文件名与内容）。
    pub fn store(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        // 通用标志第 11 位：文件名是 UTF-8
        const UTF8: u16 = 1 << 11;
        for (name, data) in files {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let (size, name_len) = (data.len() as u32, name.len() as u16);
            let header = |sig: u32, central: bool| {
                let mut h = Vec::new();
                h.extend(sig.to_le_bytes());
                if central {
                    h.extend(20u16.to_le_bytes()); // 生成的版本
                }
                h.extend(20u16.to_le_bytes()); // 需要的版本
                h.extend(UTF8.to_le_bytes());
                h.extend(0u16.to_le_bytes()); // 不压缩
                h.extend(0u16.to_le_bytes()); // 时间
                h.extend(0x21u16.to_le_bytes()); // 日期：1980-01-01
                h.extend(crc.to_le_bytes());
                h.extend(size.to_le_bytes());
                h.extend(size.to_le_bytes());
                h.extend(name_len.to_le_bytes());
                h.extend(0u16.to_le_bytes()); // 扩展字段长度
                if central {
                    h.extend(0u16.to_le_bytes()); // 注释长度
                    h.extend(0u16.to_le_bytes()); // 磁盘编号
                    h.extend(0u16.to_le_bytes()); // 内部属性
                    h.extend(0u32.to_le_bytes()); // 外部属性
                    h.extend(offset.to_le_bytes());
                }
                h.extend(name.as_bytes());
                h
            };
            out.extend(header(0x0403_4b50, false));
            out.extend(data);
            central.extend(header(0x0201_4b50, true));
        }
        let central_offset = out.len() as u32;
        let central_len = central.len() as u32;
        out.extend(central);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend(central_len.to_le_bytes());
        out.extend(central_offset.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    /// 读出 `store` 写的 zip（测试用）：文件名与内容。
    pub fn read(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
        let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let mut out = Vec::new();
        let mut i = 0;
        while i + 30 <= bytes.len() && u32_at(i) == 0x0403_4b50 {
            let (size, name_len, extra) = (u32_at(i + 18), u16_at(i + 26), u16_at(i + 28));
            let name = String::from_utf8_lossy(&bytes[i + 30..i + 30 + name_len]).to_string();
            let start = i + 30 + name_len + extra;
            let data = bytes[start..start + size].to_vec();
            assert_eq!(super::zip::crc32(&data), u32_at(i + 14) as u32, "{name} 的校验值");
            out.push((name, data));
            i = start + size;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_paths_and_game_content_are_removed() {
        let r = Redactor::new(
            &["my-very-secret-key".into()],
            &[("/home/alice/.local/share/xqlink".into(), "<数据目录>"), ("C:\\Users\\alice".into(), "~")],
            &["我的对局".into(), "王小明".into()],
            false,
        );
        let text = r#"key=my-very-secret-key Authorization: Bearer abcdef123456 sk-abcdefghij
url https://bob:pw@api.example.com/v1/chat?key=zzz path /home/alice/.local/share/xqlink/xqlink.redb
"path":"C:\\Users\\alice\\AppData" 会话“我的对局” 玩家 王小明 fen rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w
着法 炮二平五 马8进7 以及 h2e2 h9g7"#;
        let out = r.text(text);
        for gone in ["my-very-secret-key", "abcdef123456", "abcdefghij", "bob:pw", "key=zzz", "alice", "我的对局", "王小明", "rnbakabnr", "炮二平五", "马8进7", "h2e2", "h9g7"] {
            assert!(!out.contains(gone), "{gone} 没有清除：{out}");
        }
        // 编号与校验值中像着法的片段不改动
        let ids = r.text("会话 3f2a1c9e-a1b2-4c3d-8e9f-0a1b2c3d4e5f 指纹 9fa1b2c3d4");
        assert_eq!(ids, "会话 3f2a1c9e-a1b2-4c3d-8e9f-0a1b2c3d4e5f 指纹 9fa1b2c3d4");
        assert!(out.contains("api.example.com/v1/chat"), "保留接口的主机与路径：{out}");
        assert!(out.contains("<数据目录>/xqlink.redb"), "{out}");
        // 选择包含棋谱内容时保留局面与着法，但密钥仍然清除
        let keep = Redactor::new(&["my-very-secret-key".into()], &[], &["我的对局".into()], true).text(text);
        assert!(keep.contains("炮二平五") && keep.contains("我的对局") && !keep.contains("my-very-secret-key"), "{keep}");
    }

    #[test]
    fn the_zip_reads_back() {
        let files = vec![("manifest.json".to_string(), b"{}".to_vec()), ("logs/运行.log".to_string(), "第一行\n".as_bytes().to_vec())];
        assert_eq!(zip::read(&zip::store(&files)), files);
    }
}
