//! 带版本号的配置。旧版配置会迁移；无法解析时先备份原文件再使用默认值，并把原因告诉界面。
//! 时间一律使用整数毫秒。

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tracing::warn;
use xq_core::study::RuleSet;

pub const CONFIG_VERSION: u32 = 1;
pub const REPETITION_RULES: [&str; 6] = ["ChineseRule", "AsianRule", "SkyRule", "ComputerRule", "YitianRule", "AllowChase"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub log_level: String,
    pub engine: EngineConfig,
    pub analysis: AnalysisConfig,
    pub capture: CaptureConfig,
    pub cloud: CloudConfig,
    pub coach: CoachConfig,
    pub review: ReviewConfig,
}

/// 复习的安排与学习档案的自动备份。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewConfig {
    /// 复习间隔的档位（天，从小到大）：独立选对后标为已理解时升一档，其余情况用第一档。
    /// 这是可以调整的初始设计，不是对所有人都最合适的间隔。
    pub intervals_days: Vec<u32>,
    /// 自动备份学习档案保留的份数；0 表示不自动备份。
    pub backup_keep: u32,
}

impl Default for ReviewConfig {
    fn default() -> Self { Self { intervals_days: vec![1, 3, 7, 14, 30], backup_keep: 7 } }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    /// 自定义引擎程序；为空时使用安装包自带的 Pikafish。
    pub binary: Option<PathBuf>,
    pub nnue: Option<PathBuf>,
    pub threads: u32,
    pub hash_mb: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    pub quick_time_ms: u64,
    pub quick_depth: u32,
    pub deep_time_ms: u64,
    pub deep_depth: u32,
    pub multipv: u32,
    pub repetition_rule: String,
    pub sixty_move_rule: bool,
    /// 记录走子后自动在后台生成讲解。
    pub auto_explain: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub interval_ms: u64,
    pub stable_frames: u32,
    pub unexplained_frames: u32,
    pub confidence_threshold: f32,
    /// 自定义识别模型；为空时使用安装包自带的模型。
    pub model: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CloudConfig {
    pub enabled: bool,
    pub timeout_ms: u64,
}

/// 语言模型服务的接口类型。三者的结构化输出参数不同，分别适配。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoachProvider {
    /// OpenAI Chat Completions，使用 JSON Schema 结构化输出。
    Openai,
    /// DeepSeek（OpenAI 兼容接口），使用 JSON 输出模式。
    Deepseek,
    /// 本机 Ollama `/api/chat`，在 `format` 中提供 JSON Schema。
    Ollama,
}

impl CoachProvider {
    pub fn default_base_url(self) -> &'static str {
        match self {
            CoachProvider::Openai => "https://api.openai.com/v1",
            CoachProvider::Deepseek => "https://api.deepseek.com",
            CoachProvider::Ollama => "http://127.0.0.1:11434",
        }
    }

    pub fn needs_key(self) -> bool { self != CoachProvider::Ollama }

    pub fn label(self) -> &'static str {
        match self {
            CoachProvider::Openai => "OpenAI",
            CoachProvider::Deepseek => "DeepSeek",
            CoachProvider::Ollama => "Ollama",
        }
    }
}

/// AI 讲解面向的学习者水平。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    Beginner,
    Intermediate,
    Advanced,
}

/// AI 讲解的详细程度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    Brief,
    Normal,
    Deep,
}

/// 何时自动生成 AI 讲解。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoachScope {
    /// 只在用户点击时生成。
    Off,
    /// 查看某一着时自动生成（已有有效缓存时直接使用）。
    Viewed,
}

/// AI 讲解设置。密钥不在这里：`credential_ref` 只是引用（`store:coach` 表示保存在后台凭据文件，
/// `env:变量名` 表示从环境变量读取），配置随状态返回界面时不会带出密钥。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CoachConfig {
    pub enabled: bool,
    pub provider: CoachProvider,
    /// 为空时使用该接口的默认地址。
    pub base_url: String,
    pub model: String,
    pub credential_ref: Option<String>,
    pub timeout_ms: u64,
    pub max_output_tokens: u32,
    pub max_parallel_requests: u32,
    /// 每天（UTC）最多调用次数，0 表示不限。
    pub daily_request_budget: u32,
    pub auto_explain_scope: CoachScope,
    pub audience: Audience,
    pub detail: Detail,
}

impl Default for CoachConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: CoachProvider::Deepseek,
            base_url: String::new(),
            model: String::new(),
            credential_ref: None,
            timeout_ms: 60_000,
            max_output_tokens: 1500,
            max_parallel_requests: 2,
            daily_request_budget: 200,
            auto_explain_scope: CoachScope::Off,
            audience: Audience::Beginner,
            detail: Detail::Normal,
        }
    }
}

impl CoachConfig {
    /// 两份设置是否指向同一个模型服务（启用状态、接口、地址、模型、凭据引用、输出上限）。
    /// 不同时，已排队但尚未发出的请求必须停止，不能再用旧设置发送。
    pub fn same_service(&self, other: &CoachConfig) -> bool {
        self.enabled == other.enabled
            && self.provider == other.provider
            && self.base_url() == other.base_url()
            && self.model == other.model
            && self.credential_ref == other.credential_ref
            && self.max_output_tokens == other.max_output_tokens
    }

    pub fn base_url(&self) -> String {
        let url = self.base_url.trim().trim_end_matches('/');
        if url.is_empty() { self.provider.default_base_url().to_string() } else { url.to_string() }
    }
}

fn default_threads() -> u32 {
    let cores = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(2);
    (cores / 2).clamp(1, 4)
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            log_level: "info".into(),
            engine: EngineConfig::default(),
            analysis: AnalysisConfig::default(),
            capture: CaptureConfig::default(),
            cloud: CloudConfig::default(),
            coach: CoachConfig::default(),
            review: ReviewConfig::default(),
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self { Self { binary: None, nnue: None, threads: default_threads(), hash_mb: 128 } }
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            quick_time_ms: 1000,
            quick_depth: 20,
            deep_time_ms: 5000,
            deep_depth: 30,
            multipv: 3,
            repetition_rule: "ChineseRule".into(),
            sixty_move_rule: true,
            auto_explain: true,
        }
    }
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self { interval_ms: 150, stable_frames: 3, unexplained_frames: 6, confidence_threshold: 0.8, model: None }
    }
}

impl Default for CloudConfig {
    fn default() -> Self { Self { enabled: false, timeout_ms: 5000 } }
}

impl AppConfig {
    pub fn rules(&self) -> RuleSet {
        RuleSet { repetition_rule: self.analysis.repetition_rule.clone(), sixty_move_rule: self.analysis.sixty_move_rule }
    }

    /// 把越界值调整到允许范围，返回调整说明。
    pub fn normalize(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let mut clamp_u64 = |name: &str, v: &mut u64, lo: u64, hi: u64| {
            let c = (*v).clamp(lo, hi);
            if c != *v {
                notes.push(format!("{name} {} 超出范围，已调整为 {c}", *v));
                *v = c;
            }
        };
        clamp_u64("快速分析时间(毫秒)", &mut self.analysis.quick_time_ms, 100, 60_000);
        clamp_u64("深入分析时间(毫秒)", &mut self.analysis.deep_time_ms, 200, 600_000);
        clamp_u64("截图间隔(毫秒)", &mut self.capture.interval_ms, 50, 5_000);
        clamp_u64("云库超时(毫秒)", &mut self.cloud.timeout_ms, 500, 60_000);
        clamp_u64("AI 讲解超时(毫秒)", &mut self.coach.timeout_ms, 2_000, 300_000);
        let mut clamp_u32 = |name: &str, v: &mut u32, lo: u32, hi: u32| {
            let c = (*v).clamp(lo, hi);
            if c != *v {
                notes.push(format!("{name} {} 超出范围，已调整为 {c}", *v));
                *v = c;
            }
        };
        clamp_u32("线程数", &mut self.engine.threads, 1, 1024);
        clamp_u32("哈希表(MB)", &mut self.engine.hash_mb, 1, 65_536);
        clamp_u32("快速分析深度", &mut self.analysis.quick_depth, 1, 100);
        clamp_u32("深入分析深度", &mut self.analysis.deep_depth, 1, 100);
        clamp_u32("候选数", &mut self.analysis.multipv, 1, 5);
        clamp_u32("AI 讲解最大输出", &mut self.coach.max_output_tokens, 300, 8_000);
        clamp_u32("AI 讲解并发数", &mut self.coach.max_parallel_requests, 1, 4);
        clamp_u32("稳定帧数", &mut self.capture.stable_frames, 1, 30);
        clamp_u32("自动备份份数", &mut self.review.backup_keep, 0, 100);
        clamp_u32("无法解释时等待帧数", &mut self.capture.unexplained_frames, 0, 100);
        if !(0.3..=0.99).contains(&self.capture.confidence_threshold) {
            notes.push("识别置信度阈值超出范围，已恢复为 0.8".into());
            self.capture.confidence_threshold = 0.8;
        }
        if !REPETITION_RULES.contains(&self.analysis.repetition_rule.as_str()) {
            notes.push(format!("未知的重复局面规则 {}，已改为 ChineseRule", self.analysis.repetition_rule));
            self.analysis.repetition_rule = "ChineseRule".into();
        }
        self.coach.model = self.coach.model.trim().to_string();
        if let Some(r) = &self.coach.credential_ref
            && !(r == "store:coach" || r.strip_prefix("env:").is_some_and(|n| !n.is_empty()))
        {
            notes.push("AI 讲解的凭据引用无效，已清除；请重新填写密钥".into());
            self.coach.credential_ref = None;
        }
        let mut days: Vec<u32> = self.review.intervals_days.iter().copied().filter(|d| (1..=365).contains(d)).collect();
        days.sort_unstable();
        days.dedup();
        if days.is_empty() {
            notes.push("复习间隔无效，已恢复为 1、3、7、14、30 天".into());
            days = ReviewConfig::default().intervals_days;
        } else if days != self.review.intervals_days {
            notes.push(format!("复习间隔只保留 1～365 天、从小到大排列，已调整为 {days:?} 天"));
        }
        self.review.intervals_days = days;
        if !["error", "warn", "info", "debug", "trace"].contains(&self.log_level.as_str()) {
            self.log_level = "info".into();
        }
        self.version = CONFIG_VERSION;
        notes
    }
}

/// 0.1.x 版本的配置格式。
#[derive(Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    timer_interval: Option<u64>,
    #[serde(default)]
    engine: Option<LegacyEngine>,
}

#[derive(Deserialize)]
struct LegacyEngine {
    depth: Option<u32>,
    time: Option<u64>,
    threads: Option<u32>,
    hash: Option<u32>,
    chessdb_enabled: Option<bool>,
    chessdb_timeout: Option<u64>,
}

fn migrate_legacy(legacy: LegacyConfig, notes: &mut Vec<String>) -> AppConfig {
    let mut config = AppConfig::default();
    if let Some(interval) = legacy.timer_interval {
        config.capture.interval_ms = interval;
    }
    if let Some(e) = legacy.engine {
        if let Some(depth) = e.depth {
            config.analysis.deep_depth = depth;
        }
        if let Some(mut time) = e.time {
            // 旧版前端已换算为毫秒，后端又乘了 1000；超过 10 分钟的值按这一错误还原
            if time > 600_000 {
                let fixed = time / 1000;
                notes.push(format!("旧配置中的搜索时间 {time} 毫秒是重复换算单位造成的，已还原为 {fixed} 毫秒"));
                time = fixed;
            }
            config.analysis.deep_time_ms = time;
        }
        if let Some(threads) = e.threads {
            config.engine.threads = threads;
        }
        if let Some(hash) = e.hash {
            config.engine.hash_mb = hash;
        }
        if let Some(enabled) = e.chessdb_enabled {
            config.cloud.enabled = enabled;
        }
        if let Some(timeout) = e.chessdb_timeout {
            config.cloud.timeout_ms = timeout.max(1) * 1000;
        }
    }
    notes.push("已从旧版本配置迁移设置".into());
    config
}

pub struct LoadedConfig {
    pub config: AppConfig,
    pub path: PathBuf,
    /// 需要告诉用户的迁移、恢复或调整说明。
    pub notes: Vec<String>,
}

fn backup(path: &Path, notes: &mut Vec<String>) {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let target = path.with_extension(format!("json.broken-{stamp}"));
    match fs::rename(path, &target) {
        Ok(()) => notes.push(format!("原配置文件已备份为 {}", target.display())),
        Err(e) => notes.push(format!("备份原配置文件失败：{e}")),
    }
}

/// 读取配置。`legacy` 为旧版本的配置文件位置。
pub fn load(dir: &Path, legacy: Option<&Path>) -> LoadedConfig {
    let path = dir.join("config.json");
    let mut notes = Vec::new();
    let mut config = if path.exists() {
        match fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| parse(&text, &mut notes)) {
            Ok(c) => c,
            Err(e) => {
                warn!("配置文件无法读取：{e}");
                notes.push(format!("配置文件无法读取（{e}），已使用默认设置"));
                backup(&path, &mut notes);
                AppConfig::default()
            }
        }
    } else if let Some(old) = legacy.filter(|p| p.exists()) {
        match fs::read_to_string(old).map_err(|e| e.to_string()).and_then(|text| parse(&text, &mut notes)) {
            Ok(c) => c,
            Err(e) => {
                notes.push(format!("旧版本配置无法读取（{e}），已使用默认设置"));
                AppConfig::default()
            }
        }
    } else {
        AppConfig::default()
    };
    notes.extend(config.normalize());
    let loaded = LoadedConfig { config, path, notes };
    if let Err(e) = save(&loaded.path, &loaded.config) {
        warn!("保存配置失败：{e}");
    }
    loaded
}

/// 只读取配置，不写回、不备份，也不产生提示（用于服务启动前读取个别设置）。
pub fn peek(dir: &Path, legacy: Option<&Path>) -> AppConfig {
    let path = dir.join("config.json");
    let source = if path.exists() { Some(path) } else { legacy.filter(|p| p.exists()).map(Path::to_path_buf) };
    let mut ignored = Vec::new();
    let mut config = source
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|text| parse(&text, &mut ignored).ok())
        .unwrap_or_default();
    config.normalize();
    config
}

fn parse(text: &str, notes: &mut Vec<String>) -> Result<AppConfig, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("JSON 格式错误：{e}"))?;
    if value.get("version").is_some() {
        let config: AppConfig = serde_json::from_value(value).map_err(|e| format!("字段不兼容：{e}"))?;
        if config.version > CONFIG_VERSION {
            return Err(format!("配置来自更新的版本（{}）", config.version));
        }
        Ok(config)
    } else {
        let legacy: LegacyConfig = serde_json::from_value(value).map_err(|e| format!("旧版本字段不兼容：{e}"))?;
        Ok(migrate_legacy(legacy, notes))
    }
}

/// 先写临时文件再替换，避免写到一半时损坏配置。
pub fn save(path: &Path, config: &AppConfig) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(config).map_err(std::io::Error::other)?)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_time_bug_is_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("old.json");
        fs::write(
            &legacy,
            r#"{"loglevel":"INFO","timer_interval":100,"confirm_interval":200,"engine":{"depth":20,"time":5000000,"threads":4,"hash":64,"show_wdl":false,"chessdb_enabled":true,"chessdb_timeout":5}}"#,
        )
        .unwrap();
        let loaded = load(&dir.path().join("new"), Some(&legacy));
        assert_eq!(loaded.config.analysis.deep_time_ms, 5000);
        assert_eq!(loaded.config.cloud.timeout_ms, 5000);
        assert!(loaded.config.cloud.enabled);
        assert!(loaded.notes.iter().any(|n| n.contains("重复换算单位")), "{:?}", loaded.notes);
        assert!(loaded.path.exists());
    }

    #[test]
    fn broken_config_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), "{not json").unwrap();
        let loaded = load(dir.path(), None);
        assert_eq!(loaded.config, AppConfig::default());
        assert!(loaded.notes.iter().any(|n| n.contains("已备份")), "{:?}", loaded.notes);
        let backups = fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains("broken")).count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn values_are_clamped_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = AppConfig::default();
        c.analysis.multipv = 9;
        c.analysis.deep_time_ms = 1500;
        save(&dir.path().join("config.json"), &c).unwrap();
        let loaded = load(dir.path(), None);
        assert_eq!(loaded.config.analysis.multipv, 5);
        assert_eq!(loaded.config.analysis.deep_time_ms, 1500);
        assert!(loaded.notes.iter().any(|n| n.contains("候选数")));
    }
}
