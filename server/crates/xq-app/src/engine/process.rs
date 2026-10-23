use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use tracing::debug;
use tracing::trace;

use super::service::Msg;

pub(super) struct EngineProcess {
    child: Child,
    stdin: ChildStdin,
}

impl EngineProcess {
    /// 启动引擎并开启读取线程：每行输出带上代次发送给服务线程，输出结束时发送 Eof。
    pub fn spawn(binary: &Path, args: &[String], generation: u64, tx: Sender<Msg>) -> io::Result<Self> {
        let mut cmd = Command::new(binary);
        cmd.args(args);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
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
        Ok(Self { child, stdin })
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
