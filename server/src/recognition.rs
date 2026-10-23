//! 屏幕截图（xcap）与棋盘识别（ONNX Runtime）。
//!
//! 模型在运行时从 `libs/models` 加载，并由 `manifest.json` 描述输入尺寸、标签顺序与阈值；
//! 预处理、标签和输出解析相互依赖，替换模型时应整体更新清单并验证。
//! 模型或运行库缺失时识别不可用，但手动摆棋、FEN 与棋谱学习不受影响。

use std::io::Cursor;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

/// 模型清单：与模型训练时的约定保持一致。
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ModelManifest {
    pub file: String,
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
    pub sha256: Option<String>,
}

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

/// 每类棋子在棋盘上的最大数量，用于过滤重复检测。
fn class_limit(label: char) -> usize {
    match label.to_ascii_lowercase() {
        'k' | '0' => 1,
        'p' => 5,
        _ => 2,
    }
}

struct Model {
    manifest: ModelManifest,
    path: PathBuf,
    session: OnceLock<Result<ort::session::Session, String>>,
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
    fn session(&self) -> Result<&ort::session::Session, String> {
        self.session.get_or_init(|| create_session(&self.path, &self.manifest)).as_ref().map_err(|e| e.clone())
    }

    fn status(&self) -> RecognitionStatus {
        let model = format!("{}（{}）", self.manifest.file, self.manifest.version);
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
        match self.session.get() {
            Some(Err(e)) => RecognitionStatus { available: false, model, detail: e.clone() },
            Some(Ok(_)) => RecognitionStatus { available: true, model, detail: "识别模型已加载".into() },
            None => RecognitionStatus { available: true, model, detail: "首次识别时加载模型".into() },
        }
    }

    /// 在模型输入坐标系下返回检测框。
    fn detect(&self, image: &RgbaImage) -> Result<Vec<Detection>, String> {
        let session = self.session()?;
        let (w, h) = (self.manifest.input_width as usize, self.manifest.input_height as usize);
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
        let inputs = ort::inputs![self.manifest.input_name.as_str() => input.view()].map_err(|e| e.to_string())?;
        let outputs = session.run(inputs).map_err(|e| format!("模型推理失败：{e}"))?;
        let output = outputs
            .get(self.manifest.output_name.as_str())
            .ok_or_else(|| format!("模型没有名为 {} 的输出", self.manifest.output_name))?;
        let tensor = output.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let labels: Vec<char> = self.manifest.labels.chars().collect();
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
            if confidence < self.manifest.confidence {
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
        Ok(nms(dets, self.manifest.iou))
    }
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
    model: Arc<Model>,
}

impl XcapBackend {
    /// `model_dir` 下可选的 manifest.json 描述模型；`override_path` 为用户指定的模型文件。
    pub fn new(model_dir: &Path, override_path: Option<PathBuf>) -> Self {
        let manifest_path = model_dir.join("manifest.json");
        let manifest = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|text| match serde_json::from_str::<ModelManifest>(&text) {
                Ok(m) => Some(m),
                Err(e) => {
                    tracing::warn!("模型清单 {} 无法解析，使用默认设置：{e}", manifest_path.display());
                    None
                }
            })
            .unwrap_or_default();
        let path = override_path.unwrap_or_else(|| model_dir.join(&manifest.file));
        Self { model: Arc::new(Model { manifest, path, session: OnceLock::new() }) }
    }
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
    fn status(&self) -> RecognitionStatus { self.model.status() }

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
        let (suggested, note) = match self.model.detect(&image) {
            Ok(dets) => {
                let m = &self.model.manifest;
                match capture::board_crop(&dets, m.input_width as f32, m.input_height as f32, image.width(), image.height()) {
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
        let model = self.model.clone();
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
    let dets = model.detect(&crop).map_err(FrameError::Recognition)?;
    let m = &model.manifest;
    Ok(Frame { observation: capture::observe(&dets, m.input_width as f32, m.input_height as f32) })
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
