//! 逐着讲解。
//!
//! 讲解由三类证据组成：规则计算的棋盘事实、引擎变化支持的判断、注明不确定性的位置性判断。
//! 文字由模板生成，每条结论都引用证据编号，便于界面高亮和回放，也便于后续接入语言模型时校验。

pub mod compare;
pub mod compose;
pub mod describe;
pub mod facts;

pub use compose::*;
