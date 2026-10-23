//! 导入导出、诊断信息与可选的云库查询。

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use xq_core::cloud;
use xq_core::cloud::CloudOutcome;
use xq_core::fen;
use xq_core::notation;
use xq_core::study::Study;

use crate::api::CreateKind;
use crate::api::CreateStudyRequest;
use crate::api::ImportResponse;
use crate::api::NodeRef;
use crate::capture::CaptureState;
use crate::engine::EngineState;
use crate::events;
use crate::records::CloudRecord;
use crate::records::ExplanationRecord;
use crate::service::APP_VERSION;
use crate::service::ApiResult;
use crate::service::AppService;
use crate::service::now_ms;

const EXPORT_FORMAT: &str = "xqlink-study";
const EXPORT_VERSION: u32 = 1;
const CLOUD_URL: &str = "https://www.chessdb.cn/chessdb.php";

/// 完整学习数据：棋谱树、分支、标注与讲解。
#[derive(Serialize, Deserialize)]
struct StudyExport {
    format: String,
    version: u32,
    app_version: String,
    exported_at: i64,
    study: Study,
    explanations: Vec<ExplanationRecord>,
}

#[derive(Serialize)]
struct CloudEvent<'a> {
    study_id: &'a str,
    node_id: u32,
    record: Option<&'a CloudRecord>,
    error: Option<String>,
}

fn looks_like_bare_fen(text: &str) -> bool {
    let t = text.trim();
    !t.contains('\n') && !t.contains("moves") && t.split_whitespace().next().is_some_and(|f| f.matches('/').count() == 9)
}

impl AppService {
    pub fn export_study(&self, id: &str) -> ApiResult<String> {
        let study = self.inner.with_study(id, |s| Ok(s.clone()))?;
        let explanations = self.inner.store.explanations_for(id)?;
        let export = StudyExport {
            format: EXPORT_FORMAT.into(),
            version: EXPORT_VERSION,
            app_version: APP_VERSION.into(),
            exported_at: now_ms(),
            study,
            explanations,
        };
        serde_json::to_string_pretty(&export).map_err(|e| e.to_string().into())
    }

    pub fn export_study_to_file(&self, id: &str, path: &Path) -> ApiResult<()> {
        let text = self.export_study(id)?;
        std::fs::write(path, text).map_err(|e| format!("写入 {} 失败：{e}", path.display()).into())
    }

    /// 导入文本：学习数据文件、FEN 或棋谱。
    pub fn import_text(&self, text: &str, name: Option<String>) -> ApiResult<ImportResponse> {
        let trimmed = text.trim();
        if trimmed.starts_with('{') {
            let export: StudyExport =
                serde_json::from_str(trimmed).map_err(|e| format!("学习数据文件无法解析：{e}"))?;
            if export.format != EXPORT_FORMAT {
                return Err("不是本应用导出的学习数据文件".into());
            }
            if export.version > EXPORT_VERSION {
                return Err("学习数据文件来自更新的版本，请升级应用后再导入".into());
            }
            let mut study = export.study;
            // 使用新的编号，避免覆盖已有会话
            study.id = uuid::Uuid::new_v4().to_string();
            if let Some(n) = name {
                study.name = n;
            }
            study.live_node = None;
            self.inner.store.save_study(&study)?;
            for mut record in export.explanations {
                if study.nodes.contains_key(&record.node_id) {
                    record.study_id = study.id.clone();
                    record.pending = false;
                    self.inner.store.save_explanation(&record)?;
                }
            }
            let view = self.load_study(&study.id)?;
            return Ok(ImportResponse { study: view, notes: vec!["已导入学习数据（含分支、标注与讲解）".into()], stopped: None });
        }
        let kind = if looks_like_bare_fen(trimmed) {
            CreateKind::Fen { fen: trimmed.to_string(), side: None }
        } else {
            CreateKind::Record { text: trimmed.to_string() }
        };
        self.create_study(CreateStudyRequest { name, kind, orientation: None, user_side: None })
    }

    pub fn import_file(&self, path: &Path) -> ApiResult<ImportResponse> {
        let bytes = std::fs::read(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
        let text = String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 文本，请先转换编码后再导入")?;
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string());
        self.import_text(&text, name)
    }

    /// 便于用户反馈问题的诊断信息。
    pub fn diagnostics(&self) -> String {
        let status = self.status();
        let cfg = &status.config;
        let engine = match &status.engine {
            EngineState::Ready { identity, notes, .. } => {
                let mut s = format!("就绪 · {} · 网络 {} · 评分 {}", identity.name, identity.nnue, identity.score_type);
                if !notes.is_empty() {
                    s.push_str(&format!("（{}）", notes.join("；")));
                }
                s
            }
            EngineState::Starting => "启动中".into(),
            EngineState::Failed { reason } => format!("不可用：{reason}"),
            EngineState::Stopped => "已停止".into(),
        };
        let capture = match &status.capture {
            CaptureState::Idle => "未连接".to_string(),
            CaptureState::Locating { window, message } => {
                format!("定位中（{}）{}", window.title, message.as_deref().unwrap_or(""))
            }
            CaptureState::AwaitingStart { window, .. } => format!("等待确认起始局面（{}）", window.title),
            CaptureState::Tracking { window, .. } => format!("记录中（{}）", window.title),
            CaptureState::AwaitingConfirmation { window, reason, .. } => format!("等待确认（{}）：{reason}", window.title),
            CaptureState::Stopped { stage, message, .. } => format!("已停止（{stage}）：{message}"),
        };
        let mut lines = vec![
            format!("xqlink {}", status.version),
            format!("系统：{} {}", std::env::consts::OS, std::env::consts::ARCH),
            format!("引擎：{engine}"),
            format!(
                "识别：{}（{}）{}",
                if status.recognition.available { "可用" } else { "不可用" },
                status.recognition.model,
                status.recognition.detail
            ),
            format!("屏幕连接：{capture}"),
            format!(
                "分析设置：线程 {} · 哈希 {} MB · 快速 {} ms/深度 {} · 深入 {} ms/深度 {} · 候选 {} · 规则 {} · 六十步 {}",
                cfg.engine.threads,
                cfg.engine.hash_mb,
                cfg.analysis.quick_time_ms,
                cfg.analysis.quick_depth,
                cfg.analysis.deep_time_ms,
                cfg.analysis.deep_depth,
                cfg.analysis.multipv,
                cfg.analysis.repetition_rule,
                cfg.analysis.sixty_move_rule
            ),
            format!("云库：{}", if cfg.cloud.enabled { "开启" } else { "关闭" }),
            // 只说明密钥是否已配置，不输出密钥与服务地址
            format!(
                "AI 讲解：{} · {} · 密钥{} · 今日调用 {}/{}{}",
                if status.coach.enabled { "开启" } else { "关闭" },
                status.coach.model_label,
                if status.coach.key_configured { "已配置" } else { "未配置" },
                status.coach.used_today,
                status.coach.daily_budget,
                status.coach.reason.as_ref().map(|r| format!("（{r}）")).unwrap_or_default()
            ),
            format!("数据目录：{}", status.data_dir),
            format!("日志目录：{}", status.log_dir),
        ];
        if !status.notices.is_empty() {
            lines.push("最近的提示：".into());
            lines.extend(status.notices.iter().map(|n| format!("  - {n}")));
        }
        lines.join("\n")
    }

    /// 查询云库。结果通过事件返回，并保存来源与查询时间。
    pub fn cloud_lookup(&self, r: NodeRef) -> ApiResult<()> {
        let cfg = self.inner.config_snapshot().cloud;
        if !cfg.enabled {
            return Err("云库查询未开启，可在设置中开启".into());
        }
        let study = self.inner.with_study(&r.study_id, |s| Ok(s.clone()))?;
        let pos = study.position(r.node_id)?;
        let fen_text = fen::position_fen(&pos);
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let result = query_cloud(&fen_text, cfg.timeout_ms).map(|outcome| {
                let pv_notation = match &outcome {
                    CloudOutcome::Moves { pv, .. } => notation::line_to_chinese(&pos, pv),
                    _ => Vec::new(),
                };
                CloudRecord { fen: fen_text.clone(), outcome, pv_notation, source: "chessdb.cn 云库".into(), queried_at: now_ms() }
            });
            let (record, error) = match result {
                Ok(record) => {
                    if let Err(e) = inner.store.save_cloud(&record) {
                        tracing::warn!("保存云库结果失败：{e}");
                    }
                    (Some(record), None)
                }
                Err(e) => (None, Some(e)),
            };
            events::emit(inner.events.as_ref(), events::CLOUD_RESULT, &CloudEvent {
                study_id: &r.study_id,
                node_id: r.node_id,
                record: record.as_ref(),
                error,
            });
        });
        Ok(())
    }
}

fn query_cloud(fen_text: &str, timeout_ms: u64) -> Result<CloudOutcome, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .user_agent(format!("xqlink/{APP_VERSION} (+https://github.com/atopx/chessboard)"))
        .build()
        .map_err(|e| format!("无法创建网络连接：{e}"))?;
    let response = client
        .get(CLOUD_URL)
        .query(&[("action", "querypv"), ("board", fen_text)])
        .send()
        .map_err(|e| if e.is_timeout() { "云库查询超时".to_string() } else { format!("云库查询失败：{e}") })?;
    if !response.status().is_success() {
        return Err(format!("云库返回 HTTP {}", response.status()));
    }
    let text = response.text().map_err(|e| format!("读取云库响应失败：{e}"))?;
    cloud::parse_querypv(&text)
}
