//! 屏幕截图（xcap）与棋盘识别（ONNX Runtime）。
//!
//! 模型在运行时从 `libs/models` 加载，并由 `manifest.json` 描述输入尺寸、标签顺序与阈值；
//! 预处理、标签和输出解析相互依赖，替换模型时应整体更新清单并验证。
//! 模型或运行库缺失时识别不可用，但手动摆棋、FEN 与棋谱学习不受影响。
//!
//! 清单不成立时识别不可用，不改用默认设置：标签顺序不对时不会报错，而是把一种棋子认成另一种。
//! - 自带模型：使用 `libs/models/manifest.json`；没有清单时使用自带模型的约定（`ModelManifest::default`）；
//! - 设置中指定的模型：使用它旁边的 `<文件名>.manifest.json`，或同目录 `manifest.json` 中 `file` 为它的那份；
//!   都没有时，只有校验值与自带清单中的 `sha256` 一致（同一个模型）才沿用自带的清单，否则不可用；
//! - 加载失败后不会一直沿用失败的结果：模型文件变化、或用户再次截图时重新加载；在设置中换了模型文件时立即换用。

use std::io::Cursor;
use std::panic::AssertUnwindSafe;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;

use base64::Engine as _;
use ndarray::Array4;
use serde::Deserialize;
use xcap::image::DynamicImage;
use xcap::image::RgbaImage;
use xcap::image::imageops;
use xcap::image::imageops::FilterType;
use xq_app::capture::CaptureBackend;
use xq_app::capture::Frame;
use xq_app::capture::FrameError;
use xq_app::capture::FrameSource;
use xq_app::capture::RecognitionStatus;
use xq_app::capture::Snapshot;
use xq_app::capture::WindowInfo;
use xq_core::Piece;
use xq_core::capture;
use xq_core::capture::DetectedClass;
use xq_core::capture::Detection;
use xq_core::capture::Rect;

#[cfg(not(feature = "rotate"))]
const DEFAULT_MODEL: &str = "large.onnx";
#[cfg(feature = "rotate")]
const DEFAULT_MODEL: &str = "rotate.onnx";

/// 模型清单：与模型训练时的约定保持一致。约定的各项都必须写明（不按默认值补齐）。
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ModelManifest {
    pub file: String,
    #[serde(default = "unknown_version")]
    pub version: String,
    pub input_width: u32,
    pub input_height: u32,
    pub input_name: String,
    pub output_name: String,
    /// 类别顺序，'0' 表示棋盘，其余为 FEN 棋子字母。
    pub labels: String,
    pub confidence: f32,
    pub iou: f32,
    /// 期望的 SHA-256；给出时加载前校验。
    #[serde(default)]
    pub sha256: Option<String>,
}

fn unknown_version() -> String { "unknown".into() }

impl Default for ModelManifest {
    fn default() -> Self {
        Self {
            file: DEFAULT_MODEL.into(),
            version: "unknown".into(),
            input_width: 640,
            input_height: 640,
            input_name: "images".into(),
            output_name: "output".into(),
            labels: "nbakrcpRNAKBCP0".into(),
            confidence: 0.7,
            iou: 0.5,
            sha256: None,
        }
    }
}

impl ModelManifest {
    /// 清单本身是否成立（与模型是否匹配在加载与推理时检查）。
    fn check(&self) -> Result<(), String> {
        if self.file.is_empty() || self.file.contains(['/', '\\']) {
            return Err("file 应是模型的文件名".into());
        }
        if !(32..=4096).contains(&self.input_width) || !(32..=4096).contains(&self.input_height) {
            return Err("输入尺寸应在 32 到 4096 之间".into());
        }
        let labels: Vec<char> = self.labels.chars().collect();
        if labels.iter().copied().collect::<HashSet<_>>().len() != labels.len() || !labels.contains(&'0') {
            return Err("labels 中的类别不能重复，且要有表示棋盘的 0".into());
        }
        if let Some(c) = labels.iter().find(|c| **c != '0' && Piece::from_fen_char(**c).is_none()) {
            return Err(format!("labels 中的 {c} 不是棋子的 FEN 字母"));
        }
        if !(self.confidence > 0.0 && self.confidence <= 1.0 && self.iou > 0.0 && self.iou <= 1.0) {
            return Err("confidence 与 iou 应在 0 与 1 之间".into());
        }
        if self.sha256.as_ref().is_some_and(|h| h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit())) {
            return Err("sha256 应是 64 位十六进制".into());
        }
        Ok(())
    }
}

/// 读取一份清单：不存在时为 `Ok(None)`；存在但无法读取、解析或不成立时为错误（不改用默认设置）。
fn read_manifest(path: &Path) -> Result<Option<ModelManifest>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("无法读取模型清单 {}：{e}", path.display())),
    };
    let manifest: ModelManifest =
        serde_json::from_str(&text).map_err(|e| format!("模型清单 {} 无法解析：{e}；识别不可用", path.display()))?;
    manifest.check().map_err(|e| format!("模型清单 {} 不成立：{e}；识别不可用", path.display()))?;
    Ok(Some(manifest))
}

/// 选出模型文件与它的清单（见模块说明）。
fn resolve(model_dir: &Path, custom: Option<&Path>) -> (PathBuf, Result<ModelManifest, String>) {
    let bundled = read_manifest(&model_dir.join("manifest.json"));
    let Some(path) = custom else {
        return match bundled {
            Ok(Some(m)) => (model_dir.join(&m.file), Ok(m)),
            Ok(None) => (model_dir.join(DEFAULT_MODEL), Ok(ModelManifest::default())),
            Err(e) => (model_dir.join(DEFAULT_MODEL), Err(e)),
        };
    };
    let path = path.to_path_buf();
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let stem = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let own = match read_manifest(&dir.join(format!("{stem}.manifest.json"))) {
        Ok(None) => read_manifest(&dir.join("manifest.json")).map(|m| m.filter(|m| m.file == name)),
        other => other,
    };
    let manifest = match own {
        Ok(Some(m)) => Ok(m),
        Err(e) => Err(e),
        // 与自带模型是同一个文件（校验值一致）：沿用自带的清单
        Ok(None) => match bundled {
            Ok(Some(m)) if m.sha256.as_ref().is_some_and(|h| sha256_hex(&path).is_ok_and(|a| a.eq_ignore_ascii_case(h))) => Ok(m),
            _ => Err(format!(
                "指定的识别模型 {} 没有清单：请在它旁边放一份 {stem}.manifest.json，写明输入尺寸、标签顺序与阈值（格式见 libs/models/README.md）",
                path.display()
            )),
        },
    };
    (path, manifest)
}

/// 每类棋子在棋盘上的最大数量，用于过滤重复检测。
fn class_limit(label: char) -> usize {
    match label.to_ascii_lowercase() {
        'k' | '0' => 1,
        'p' => 5,
        _ => 2,
    }
}

/// 模型文件的大小与修改时间：加载失败后文件变化时重新加载。
type Stamp = (u64, Option<SystemTime>);

fn stamp_of(path: &Path) -> Option<Stamp> { std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok())) }

enum SessionState {
    Unloaded,
    Ready(ort::session::Session),
    Failed { reason: String, stamp: Option<Stamp> },
}

struct Model {
    /// 清单不成立时识别不可用，原因在这里。
    manifest: Result<ModelManifest, String>,
    path: PathBuf,
    custom: Option<PathBuf>,
    state: Mutex<SessionState>,
}

fn execution_providers() -> Vec<ort::execution_providers::ExecutionProviderDispatch> {
    #[cfg(all(target_os = "windows", feature = "gpu"))]
    return vec![
        ort::execution_providers::CUDAExecutionProvider::default().build(),
        ort::execution_providers::DirectMLExecutionProvider::default().build(),
    ];
    #[cfg(all(target_os = "windows", not(feature = "gpu")))]
    return vec![];
    #[cfg(target_os = "macos")]
    return vec![ort::execution_providers::CoreMLExecutionProvider::default().build()];
    #[cfg(target_os = "linux")]
    return vec![ort::execution_providers::CUDAExecutionProvider::default().build()];
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    let bytes = std::fs::read(path).map_err(|e| format!("读取模型失败：{e}"))?;
    Ok(sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

fn create_session(path: &Path, manifest: &ModelManifest) -> Result<ort::session::Session, String> {
    if let Some(expected) = &manifest.sha256 {
        let actual = sha256_hex(path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!("模型文件校验值不符（期望 {expected}，实际 {actual}）"));
        }
    }
    // 动态加载的运行库缺失时 ort 可能直接 panic，这里转换为可显示的错误
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<ort::session::Session, String> {
        ort::init().with_execution_providers(execution_providers()).commit().map_err(|e| e.to_string())?;
        ort::session::Session::builder()
            .and_then(|b| b.with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3))
            .and_then(|b| b.commit_from_file(path))
            .map_err(|e| e.to_string())
    }));
    match result {
        Ok(Ok(session)) => Ok(session),
        Ok(Err(e)) => Err(format!("加载识别模型失败：{e}")),
        Err(_) => Err("无法加载 ONNX Runtime 运行库（缺少或版本不匹配）".into()),
    }
}

impl Model {
    fn new(model_dir: &Path, custom: Option<PathBuf>) -> Self {
        let (path, manifest) = resolve(model_dir, custom.as_deref());
        if let Err(e) = &manifest {
            tracing::warn!("{e}");
        }
        Self { manifest, path, custom, state: Mutex::new(SessionState::Unloaded) }
    }

    /// 用加载好的模型运行 `f`。还没有加载时加载；加载失败后，模型文件有变化、或 `retry`（用户再次截图）时重新加载，
    /// 连续截图中不反复加载。
    fn with_session<R>(
        &self, retry: bool, f: impl FnOnce(&ModelManifest, &ort::session::Session) -> Result<R, String>,
    ) -> Result<R, String> {
        let manifest = self.manifest.as_ref().map_err(Clone::clone)?;
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let reload = match &*state {
            SessionState::Ready(_) => false,
            SessionState::Unloaded => true,
            SessionState::Failed { stamp, .. } => retry || stamp_of(&self.path) != *stamp,
        };
        if reload {
            *state = match create_session(&self.path, manifest) {
                Ok(session) => SessionState::Ready(session),
                Err(reason) => SessionState::Failed { reason, stamp: stamp_of(&self.path) },
            };
        }
        match &*state {
            SessionState::Ready(session) => f(manifest, session),
            SessionState::Failed { reason, .. } => Err(reason.clone()),
            SessionState::Unloaded => Err("识别模型还没有加载".into()),
        }
    }

    fn status(&self) -> RecognitionStatus {
        let file = self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let model = match &self.manifest {
            Ok(m) => format!("{file}（{}）", m.version),
            Err(_) => file,
        };
        if let Err(e) = &self.manifest {
            return RecognitionStatus { available: false, model, detail: e.clone() };
        }
        if !self.path.exists() {
            return RecognitionStatus {
                available: false,
                model,
                detail: format!(
                    "未找到识别模型 {}；安装包应自带模型，从源码构建时请按 libs/models/README.md 放置模型，或在设置中指定模型文件",
                    self.path.display()
                ),
            };
        }
        // 正在识别时（持有模型）不等待，按已加载说明
        let Ok(state) = self.state.try_lock() else {
            return RecognitionStatus { available: true, model, detail: "识别模型已加载".into() };
        };
        match &*state {
            SessionState::Failed { reason, .. } => RecognitionStatus {
                available: false,
                model,
                detail: format!("{reason}（替换模型文件或再次截图时会重新加载）"),
            },
            SessionState::Ready(_) => RecognitionStatus { available: true, model, detail: "识别模型已加载".into() },
            SessionState::Unloaded => RecognitionStatus { available: true, model, detail: "首次识别时加载模型".into() },
        }
    }

    /// 返回模型输入坐标系下的检测框与输入尺寸。
    fn detect(&self, image: &RgbaImage, retry: bool) -> Result<(Vec<Detection>, (f32, f32)), String> {
        self.with_session(retry, |manifest, session| {
            let dets = detect_with(manifest, session, image)?;
            Ok((dets, (manifest.input_width as f32, manifest.input_height as f32)))
        })
    }
}

fn detect_with(manifest: &ModelManifest, session: &ort::session::Session, image: &RgbaImage) -> Result<Vec<Detection>, String> {
    let (w, h) = (manifest.input_width as usize, manifest.input_height as usize);
    let resized = DynamicImage::ImageRgba8(image.clone()).resize_exact(w as u32, h as u32, FilterType::Triangle).to_rgba8();
    let mut input = Array4::<f32>::zeros((1, 3, h, w));
    {
        let data = input.as_slice_mut().ok_or("输入张量布局异常")?;
        let plane = w * h;
        for (i, px) in resized.as_raw().as_chunks::<4>().0.iter().enumerate() {
            data[i] = px[0] as f32 / 255.0;
            data[plane + i] = px[1] as f32 / 255.0;
            data[2 * plane + i] = px[2] as f32 / 255.0;
        }
    }
    let inputs = ort::inputs![manifest.input_name.as_str() => input.view()].map_err(|e| e.to_string())?;
    let outputs = session.run(inputs).map_err(|e| format!("模型推理失败：{e}"))?;
    let output = outputs
        .get(manifest.output_name.as_str())
        .ok_or_else(|| format!("模型没有名为 {} 的输出", manifest.output_name))?;
    let tensor = output.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    let labels: Vec<char> = manifest.labels.chars().collect();
    let shape = tensor.shape().to_vec();
    // 输出为 YOLOv5 格式：[1, N, 4 + 1 + 类别数]，含 objectness
    if shape.len() != 3 || shape[2] != 5 + labels.len() {
        return Err(format!("模型输出形状 {shape:?} 与清单（{} 个类别）不符", labels.len()));
    }
    let tensor = tensor.into_dimensionality::<ndarray::Ix3>().map_err(|e| e.to_string())?;
    let mut dets = Vec::new();
    for row in tensor.index_axis(ndarray::Axis(0), 0).rows() {
        let Some((class, prob)) =
            (0..labels.len()).map(|c| (c, row[5 + c])).max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        let confidence = row[4] * prob;
        if confidence < manifest.confidence {
            continue;
        }
        let class = match labels[class] {
            '0' => DetectedClass::Board,
            c => match Piece::from_fen_char(c) {
                Some(p) => DetectedClass::Piece(p),
                None => continue,
            },
        };
        let (cx, cy, bw, bh) = (row[0], row[1], row[2], row[3]);
        dets.push(Detection { x0: cx - bw / 2.0, y0: cy - bh / 2.0, x1: cx + bw / 2.0, y1: cy + bh / 2.0, class, confidence });
    }
    Ok(nms(dets, manifest.iou))
}

fn label_of(class: DetectedClass) -> char {
    match class {
        DetectedClass::Board => '0',
        DetectedClass::Piece(p) => p.fen_char(),
    }
}

fn iou(a: &Detection, b: &Detection) -> f32 {
    let iw = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let ih = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let inter = iw * ih;
    let area = |d: &Detection| (d.x1 - d.x0) * (d.y1 - d.y0);
    let union = area(a) + area(b) - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// 跨类别非极大值抑制，并按棋规限制每类数量。
fn nms(mut dets: Vec<Detection>, threshold: f32) -> Vec<Detection> {
    dets.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut kept: Vec<Detection> = Vec::new();
    for det in dets {
        let label = label_of(det.class);
        if kept.iter().filter(|k| label_of(k.class) == label).count() >= class_limit(label) {
            continue;
        }
        // 棋盘框包含所有棋子，只与同类比较重叠
        let overlaps = kept.iter().any(|k| (k.class == DetectedClass::Board) == (det.class == DetectedClass::Board) && iou(k, &det) >= threshold);
        if !overlaps {
            kept.push(det);
        }
    }
    kept
}

pub struct XcapBackend {
    model_dir: PathBuf,
    /// 正在进行的屏幕连接持有它打开时的模型；换了模型文件之后的截图使用新的。
    model: RwLock<Arc<Model>>,
}

impl XcapBackend {
    /// `model_dir` 下的 manifest.json 描述自带模型；`override_path` 为用户指定的模型文件。
    pub fn new(model_dir: &Path, override_path: Option<PathBuf>) -> Self {
        Self { model_dir: model_dir.to_path_buf(), model: RwLock::new(Arc::new(Model::new(model_dir, override_path))) }
    }

    fn model(&self) -> Arc<Model> { self.model.read().unwrap_or_else(|p| p.into_inner()).clone() }
}

fn window_info(w: &xcap::Window) -> Option<WindowInfo> {
    let width = w.width().ok()?;
    let height = w.height().ok()?;
    if width < 50 || height < 50 || w.is_minimized().unwrap_or(false) {
        return None;
    }
    Some(WindowInfo {
        id: w.id().ok()?,
        title: w.title().unwrap_or_default(),
        app_name: w.app_name().unwrap_or_default(),
        width,
        height,
    })
}

fn find_window(id: u32) -> Result<xcap::Window, String> {
    xcap::Window::all()
        .map_err(|e| format!("无法枚举窗口：{e}"))?
        .into_iter()
        .find(|w| w.id().ok() == Some(id))
        .ok_or_else(|| "目标窗口已关闭".to_string())
}

fn encode_preview(image: &RgbaImage) -> Result<String, String> {
    const MAX_WIDTH: u32 = 1400;
    let preview = if image.width() > MAX_WIDTH {
        let height = (image.height() as f64 * MAX_WIDTH as f64 / image.width() as f64).round() as u32;
        imageops::resize(image, MAX_WIDTH, height.max(1), FilterType::Triangle)
    } else {
        image.clone()
    };
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(preview).write_to(&mut buf, xcap::image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf.into_inner())))
}

impl CaptureBackend for XcapBackend {
    fn status(&self) -> RecognitionStatus { self.model().status() }

    fn set_model(&self, path: Option<PathBuf>) {
        let mut model = self.model.write().unwrap_or_else(|p| p.into_inner());
        if model.custom != path {
            *model = Arc::new(Model::new(&self.model_dir, path));
        }
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, String> {
        let windows = xcap::Window::all().map_err(|e| format!("无法枚举窗口（可能缺少屏幕录制权限）：{e}"))?;
        let list: Vec<WindowInfo> = windows.iter().filter_map(window_info).collect();
        if list.is_empty() {
            return Err("没有找到可截取的窗口；macOS 需要在系统设置中授予屏幕录制权限".into());
        }
        Ok(list)
    }

    fn snapshot(&self, window_id: u32) -> Result<Snapshot, String> {
        let window = find_window(window_id)?;
        let info = window_info(&window).ok_or("窗口已最小化或尺寸过小")?;
        let image = window.capture_image().map_err(|e| format!("截图失败：{e}"))?;
        // 用户发起的截图：上次加载失败时再试一次
        let (suggested, note) = match self.model().detect(&image, true) {
            Ok((dets, (w, h))) => {
                match capture::board_crop(&dets, w, h, image.width(), image.height()) {
                    Some(rect) => (Some(rect), None),
                    None => (None, Some("没有自动定位到棋盘，请手动框选棋盘区域".to_string())),
                }
            }
            Err(e) => (None, Some(format!("{e}；可以先手动框选棋盘区域"))),
        };
        Ok(Snapshot {
            window: info,
            width: image.width(),
            height: image.height(),
            image: Some(encode_preview(&image)?),
            suggested,
            note,
        })
    }

    fn open(&self, window_id: u32, rect: Rect) -> Result<Box<dyn FrameSource>, String> {
        if rect.w < 90.0 || rect.h < 100.0 {
            return Err("框选的棋盘区域太小".into());
        }
        let (req_tx, req_rx) = mpsc::channel::<()>();
        let (resp_tx, resp_rx) = mpsc::channel::<Result<Frame, FrameError>>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let model = self.model();
        // xcap 的窗口句柄不能跨线程移动：由专用线程创建并持有，通过通道交换截图请求与结果
        thread::Builder::new()
            .name("capture-window".into())
            .spawn(move || {
                let window = match find_window(window_id) {
                    Ok(w) => w,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let expected = match window.capture_image() {
                    Ok(img) => (img.width(), img.height()),
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("截图失败：{e}")));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                while req_rx.recv().is_ok() {
                    let frame = grab(&window, expected, rect, &model);
                    if resp_tx.send(frame).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("无法启动截图线程：{e}"))?;
        ready_rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "截图线程没有响应".to_string())??;
        Ok(Box::new(ChannelSource { req_tx, resp_rx }))
    }
}

fn grab(window: &xcap::Window, expected: (u32, u32), rect: Rect, model: &Model) -> Result<Frame, FrameError> {
    let image = match window.capture_image() {
        Ok(img) => img,
        Err(e) => {
            let gone = xcap::Window::all().map(|all| !all.iter().any(|w| w.id().ok() == window.id().ok())).unwrap_or(false);
            return Err(if gone { FrameError::WindowGone("目标窗口已关闭".into()) } else { FrameError::Capture(format!("截图失败：{e}")) });
        }
    };
    if (image.width(), image.height()) != expected {
        return Err(FrameError::Resized(format!(
            "窗口尺寸已从 {}×{} 变为 {}×{}，请重新框选棋盘",
            expected.0,
            expected.1,
            image.width(),
            image.height()
        )));
    }
    let (x, y) = (rect.x.max(0.0) as u32, rect.y.max(0.0) as u32);
    let (w, h) = (rect.w as u32, rect.h as u32);
    if x + w > image.width() || y + h > image.height() {
        return Err(FrameError::Resized("棋盘区域超出了窗口范围，请重新框选".into()));
    }
    let crop = imageops::crop_imm(&image, x, y, w, h).to_image();
    let (dets, (w, h)) = model.detect(&crop, false).map_err(FrameError::Recognition)?;
    Ok(Frame { observation: capture::observe(&dets, w, h) })
}

struct ChannelSource {
    req_tx: mpsc::Sender<()>,
    resp_rx: mpsc::Receiver<Result<Frame, FrameError>>,
}

impl FrameSource for ChannelSource {
    fn grab(&mut self) -> Result<Frame, FrameError> {
        self.req_tx.send(()).map_err(|_| FrameError::Capture("截图线程已退出".into()))?;
        self.resp_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| FrameError::Capture("截图或识别超时".into()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) { std::fs::write(path, text).unwrap(); }

    fn manifest_json(file: &str, labels: &str, sha256: Option<&str>) -> String {
        serde_json::json!({
            "file": file, "version": "t", "input_width": 640, "input_height": 640, "input_name": "images",
            "output_name": "output", "labels": labels, "confidence": 0.7, "iou": 0.5, "sha256": sha256
        })
        .to_string()
    }

    #[test]
    fn a_manifest_that_does_not_hold_disables_recognition_instead_of_using_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        // 没有清单：自带模型使用自带的约定
        let (path, m) = resolve(&models, None);
        assert_eq!((path, m), (models.join(DEFAULT_MODEL), Ok(ModelManifest::default())));
        // 清单无法解析、缺少约定的项或标签不成立：不可用，不改用默认设置
        for bad in ["{ 不是 JSON", r#"{"file": "large.onnx"}"#, &manifest_json("large.onnx", "nbakrcpRNAKBCPn", None)] {
            write(&models.join("manifest.json"), bad);
            let (_, m) = resolve(&models, None);
            assert!(m.as_ref().is_err_and(|e| e.contains("识别不可用")), "{bad}: {m:?}");
        }
        write(&models.join("manifest.json"), &manifest_json("large.onnx", "nbakrcpRNAKBCP0", None));
        assert!(resolve(&models, None).1.is_ok());

        // 指定的模型没有自己的清单：不可用
        let custom = dir.path().join("mine.onnx");
        write(&custom, "model-bytes");
        let (path, m) = resolve(&models, Some(&custom));
        assert_eq!(path, custom);
        assert!(m.is_err_and(|e| e.contains("mine.manifest.json")));
        // 旁边有 mine.manifest.json：使用它
        write(&dir.path().join("mine.manifest.json"), &manifest_json("mine.onnx", "0RNBAKCPrnbakcp", None));
        assert_eq!(resolve(&models, Some(&custom)).1.unwrap().labels, "0RNBAKCPrnbakcp");
        // 与自带模型是同一个文件（校验值一致）：没有自己的清单时沿用自带的
        std::fs::remove_file(dir.path().join("mine.manifest.json")).unwrap();
        let hash = sha256_hex(&custom).unwrap();
        write(&models.join("manifest.json"), &manifest_json("large.onnx", "nbakrcpRNAKBCP0", Some(&hash)));
        assert!(resolve(&models, Some(&custom)).1.is_ok());
    }

    #[test]
    fn a_failed_load_is_tried_again_when_the_file_changes_or_the_user_retries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("large.onnx");
        write(&file, "不是模型");
        let model = Model::new(dir.path(), None);
        // 校验值不符：加载失败
        let expect = "0".repeat(64);
        let model = Model { manifest: Ok(ModelManifest { sha256: Some(expect), ..ModelManifest::default() }), ..model };
        assert!(model.with_session(false, |_, _| Ok(())).is_err_and(|e| e.contains("校验值不符")));
        assert!(!model.status().available && model.status().detail.contains("重新加载"));
        let failed_at = |m: &Model| match &*m.state.lock().unwrap() {
            SessionState::Failed { stamp, .. } => *stamp,
            _ => panic!("应为加载失败"),
        };
        let before = failed_at(&model);
        // 文件没有变化、也不是用户再试：不重新加载
        assert!(model.with_session(false, |_, _| Ok(())).is_err());
        assert_eq!(failed_at(&model), before);
        // 文件变化：重新加载（这里仍然失败，记下新的文件状态）
        write(&file, "换了一个文件，长度也不同");
        assert!(model.with_session(false, |_, _| Ok(())).is_err());
        assert_ne!(failed_at(&model), before);
    }
}
