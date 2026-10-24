//! 测试用的可编排 UCI 引擎：按脚本输出搜索信息，用来稳定复现候选排名交换、
//! 一轮搜索只更新部分候选、提前结束、进程退出等情况。不随应用发布。
//!
//! 脚本路径取第一个命令行参数，没有参数时读取当前目录下的 `mock-uci-script.txt`。
//! 脚本中第一个 `@go` 之前的行在握手时输出；每个 `@go` 段对应一次 `go` 命令，按顺序使用：
//! - `sleep <毫秒>`：暂停；
//! - `wait-stop`：等待收到 `stop` 再继续；
//! - `exit <退出码>`：直接退出，模拟崩溃；
//! - 其他行原样输出。
//!
//! 脚本段用完后，`go` 命令只输出 `bestmove (none)`。
//!
//! 收到的 `go` 与 `setoption` 命令追加写入当前目录下的 `mock-uci-commands.log`，测试据此检查引擎实际收到的条件。

use std::collections::VecDeque;
use std::io::BufRead;
use std::io::Write;
use std::time::Duration;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "mock-uci-script.txt".to_string());
    let script = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("无法读取脚本 {path}：{e}"));
    let mut header = Vec::new();
    let mut blocks: VecDeque<Vec<String>> = VecDeque::new();
    for line in script.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "@go" {
            blocks.push_back(Vec::new());
        } else if let Some(block) = blocks.back_mut() {
            block.push(line.to_string());
        } else {
            header.push(line.to_string());
        }
    }

    let stdin = std::io::stdin();
    let mut input = stdin.lock().lines();
    let mut out = std::io::stdout();
    let mut say = |text: &str| {
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    };
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open("mock-uci-commands.log").ok();
    while let Some(Ok(command)) = input.next() {
        let command = command.trim();
        if (command.starts_with("go") || command.starts_with("setoption"))
            && let Some(f) = log.as_mut()
        {
            let _ = writeln!(f, "{command}");
        }
        match command.split_whitespace().next().unwrap_or("") {
            "uci" => {
                say("id name MockUCI");
                say("option name Threads type spin default 1 min 1 max 64");
                say("option name Hash type spin default 16 min 1 max 1024");
                say("option name MultiPV type spin default 1 min 1 max 128");
                say("option name Repetition Rule type combo default AsianRule var AsianRule var ChineseRule");
                say("option name Sixty Move Rule type check default true");
                say("option name ScoreType type combo default Elo var Elo var PawnValueNormalized");
                for line in &header {
                    say(line);
                }
                say("uciok");
            }
            "isready" => say("readyok"),
            "quit" => return,
            "go" => {
                let Some(block) = blocks.pop_front() else {
                    say("bestmove (none)");
                    continue;
                };
                for line in block {
                    let mut parts = line.split_whitespace();
                    match parts.next() {
                        Some("sleep") => {
                            let ms = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                            std::thread::sleep(Duration::from_millis(ms));
                        }
                        Some("exit") => std::process::exit(parts.next().and_then(|v| v.parse().ok()).unwrap_or(1)),
                        Some("wait-stop") => {
                            for next in input.by_ref() {
                                match next {
                                    Ok(c) if c.trim() == "stop" => break,
                                    Ok(c) if c.trim() == "quit" => return,
                                    Ok(_) => {}
                                    Err(_) => return,
                                }
                            }
                        }
                        _ => say(&line),
                    }
                }
            }
            _ => {}
        }
    }
}
