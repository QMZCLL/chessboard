//! 故障注入点：只在启用 `failpoints` 功能的构建（测试与端到端场景）中起作用，发布版本中 `hit` 什么也不做。
//!
//! 在写入的关键位置调用 `hit("名称")?`。注入点可以设置为：
//! - `abort`：立即终止进程（模拟断电或强制结束），不运行析构，不提交进行中的事务；
//! - `error`：让这一步返回错误（模拟磁盘或数据库出错）。
//!
//! 设置来源：环境变量 `XQ_FAILPOINTS`（如 `migrate:after_studies=abort;write_atomic:before_rename=error`，
//! 用于在子进程中模拟崩溃），或测试中调用 `set`。

use std::fmt;

/// 注入的错误。
#[derive(Debug)]
pub struct Injected(pub String);

impl fmt::Display for Injected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "注入的故障：{}", self.0) }
}

impl std::error::Error for Injected {}

impl From<Injected> for std::io::Error {
    fn from(e: Injected) -> Self { std::io::Error::other(e.to_string()) }
}

#[cfg(feature = "failpoints")]
mod imp {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    use super::Injected;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Action {
        Abort,
        Error,
    }

    fn table() -> &'static Mutex<HashMap<String, Action>> {
        static TABLE: OnceLock<Mutex<HashMap<String, Action>>> = OnceLock::new();
        TABLE.get_or_init(|| {
            let mut map = HashMap::new();
            if let Ok(spec) = std::env::var("XQ_FAILPOINTS") {
                for item in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
                    match item.split_once('=') {
                        Some((name, "abort")) => {
                            map.insert(name.to_string(), Action::Abort);
                        }
                        Some((name, "error")) => {
                            map.insert(name.to_string(), Action::Error);
                        }
                        _ => panic!("XQ_FAILPOINTS 中无法识别的设置：{item}"),
                    }
                }
            }
            Mutex::new(map)
        })
    }

    /// 设置或清除一个注入点（`None` 清除）。
    pub fn set(name: &str, action: Option<Action>) {
        let mut t = table().lock().unwrap_or_else(|p| p.into_inner());
        match action {
            Some(a) => t.insert(name.to_string(), a),
            None => t.remove(name),
        };
    }

    pub fn hit(name: &str) -> Result<(), Injected> {
        let action = table().lock().unwrap_or_else(|p| p.into_inner()).get(name).copied();
        match action {
            None => Ok(()),
            Some(Action::Error) => Err(Injected(name.to_string())),
            Some(Action::Abort) => {
                // 让父进程知道终止发生在哪个注入点（标准错误不经过缓冲）
                eprintln!("failpoint abort: {name}");
                std::process::abort()
            }
        }
    }
}

#[cfg(feature = "failpoints")]
pub use imp::Action;
#[cfg(feature = "failpoints")]
pub use imp::hit;
#[cfg(feature = "failpoints")]
pub use imp::set;

#[cfg(not(feature = "failpoints"))]
#[inline(always)]
pub fn hit(_name: &str) -> Result<(), Injected> { Ok(()) }
