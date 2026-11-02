use std::collections::VecDeque;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use tracing::debug;
use tracing::trace;

use super::service::Msg;

/// 保留的错误输出行数与每行的长度。
const STDERR_LINES: usize = 20;
const STDERR_LINE_CHARS: usize = 300;

/// 引擎的错误输出（stderr）：缺少网络文件、指令集不支持、运行库出错时，原因往往只写在这里。
/// 不与 UCI 输出混在一起解析，只保留最近几行用于诊断。
#[derive(Clone, Default)]
struct StderrTail {
    lines: Arc<Mutex<VecDeque<String>>>,
    done: Arc<AtomicBool>,
}

pub(super) struct EngineProcess {
    child: Child,
    stdin: ChildStdin,
    stderr: StderrTail,
}

impl EngineProcess {
    /// 启动引擎并开启读取线程：每行输出带上代次发送给服务线程，输出结束时发送 Eof。
    pub fn spawn(binary: &Path, args: &[String], generation: u64, tx: Sender<Msg>) -> io::Result<Self> {
        let mut cmd = Command::new(binary);
        cmd.args(args);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = binary.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| io::Error::other("无法获取引擎标准输入"))?;
        let stdout = child.stdout.take().ok_or_else(|| io::Error::other("无法获取引擎标准输出"))?;
        let stderr = StderrTail::default();
        if let Some(err) = child.stderr.take() {
            let tail = stderr.clone();
            thread::Builder::new().name(format!("engine-stderr-{generation}")).spawn(move || {
                let mut reader = BufReader::new(err);
                let mut buf = Vec::new();
                while matches!(reader.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
                    let line: String = String::from_utf8_lossy(&buf).trim().chars().take(STDERR_LINE_CHARS).collect();
                    buf.clear();
                    if line.is_empty() {
                        continue;
                    }
                    debug!(target: "engine", "stderr: {line}");
                    let mut lines = tail.lines.lock().unwrap_or_else(|p| p.into_inner());
                    if lines.len() == STDERR_LINES {
                        lines.pop_front();
                    }
                    lines.push_back(line);
                }
                tail.done.store(true, Ordering::Release);
            })?;
        } else {
            stderr.done.store(true, Ordering::Release);
        }
        thread::Builder::new().name(format!("engine-reader-{generation}")).spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        // 路径等内容可能不是 UTF-8，按有损方式解码，不能当成输出结束
                        let line = String::from_utf8_lossy(&buf).trim().to_string();
                        trace!(target: "engine", "-> {line}");
                        if tx.send(Msg::Line(generation, line)).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(Msg::Eof(generation));
        })?;
        Ok(Self { child, stdin, stderr })
    }

    /// 结束进程并返回最近的错误输出（等错误输出读完，最多等半秒）。用于说明引擎为什么不可用。
    pub fn kill_and_stderr(&mut self) -> Vec<String> {
        self.kill();
        let deadline = Instant::now() + Duration::from_millis(500);
        while !self.stderr.done.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        self.stderr.lines.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned().collect()
    }

    pub fn send(&mut self, command: &str) -> io::Result<()> {
        debug!(target: "engine", "<- {command}");
        writeln!(self.stdin, "{command}")?;
        self.stdin.flush()
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// 请求引擎退出，短暂等待后强制结束。
    pub fn quit(&mut self) {
        let _ = self.send("quit");
        let deadline = Instant::now() + Duration::from_millis(1000);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        self.kill();
    }
}
