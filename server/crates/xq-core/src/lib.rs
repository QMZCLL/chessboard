//! 中国象棋领域核心。
//!
//! 这里只包含纯逻辑：棋盘与规则、FEN、中文记谱、棋谱导入、棋谱树、
//! 引擎协议解析、讲解证据与屏幕识别结果的几何换算。它不依赖窗口捕获、
//! 网络或 Tauri，因此规则和讲解可以单独测试。
//!
//! 坐标约定：标准方向红方在下，`file` 0..=8 对应 a..i，`rank` 0..=9，
//! rank 0 是红方底线。FEN 从 rank 9（黑方底线）写起。

pub mod board;
pub mod capture;
pub mod cloud;
pub mod explain;
pub mod fen;
pub mod notation;
pub mod record;
pub mod rules;
pub mod study;
pub mod types;
pub mod uci;
pub mod validate;

pub use board::{Board, Position};
pub use types::{Color, Move, Piece, PieceKind, Square};
