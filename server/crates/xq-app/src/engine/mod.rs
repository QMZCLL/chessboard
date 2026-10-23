//! 引擎服务：独立线程管理 UCI 引擎进程，通过任务队列提交分析。
//!
//! - 同一进程同时只执行一个搜索；新局面在上一次搜索结束（收到 bestmove）后才发送。
//! - 进程输出带“代次”编号，重启前的旧输出不会混入新搜索。
//! - 启动时完成 `uci`/`isready` 握手，只设置引擎实际支持的选项。
//! - 停止无响应时结束进程并重启；进程退出（输出结束）会被检测并报告，而不是无限等待。

mod process;
mod service;

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
pub use service::EngineService;
pub use service::JobSpec;
use xq_core::Move;
use xq_core::explain::EngineLine;
use xq_core::rules::GameStatus;
use xq_core::study::RuleSet;

pub type JobId = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineSpec {
    pub binary: PathBuf,
    /// 启动引擎时的命令行参数（自带的 Pikafish 不需要）。
    pub args: Vec<String>,
    pub nnue: Option<PathBuf>,
    pub threads: u32,
    pub hash_mb: u32,
    pub rules: RuleSet,
}

/// 搜索结果所对应的引擎与网络文件，用于缓存键与结果追踪。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineIdentity {
    pub name: String,
    pub nnue: String,
    pub score_type: String,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AppliedOption {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EngineState {
    Starting,
    Ready { identity: EngineIdentity, applied: Vec<AppliedOption>, notes: Vec<String> },
    Failed { reason: String },
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Background,
    Interactive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRequest {
    /// 起始局面（棋谱段起点）与此后的走法，引擎据此判断重复局面。
    pub base_fen: String,
    pub moves: Vec<Move>,
    pub multipv: u32,
    pub depth: Option<u32>,
    pub movetime_ms: Option<u64>,
    /// 只搜索这些根走法（用于分析未进入候选的实际走法）。
    pub searchmoves: Vec<Move>,
    /// 提交任务时依据的引擎与规则。执行时若引擎设置已改变，任务以 `Stale` 结束，
    /// 避免结果按旧条件写入缓存。
    pub expect: Option<EngineExpectation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineExpectation {
    pub engine: String,
    pub nnue: String,
    pub score_type: String,
    pub rules: RuleSet,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchProgress {
    pub job: JobId,
    pub depth: u32,
    pub nodes: u64,
    pub time_ms: u64,
    pub lines: Vec<EngineLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    /// 一轮完整搜索的候选（按名次）；没有完整一轮时为最后一轮的部分结果。
    pub lines: Vec<EngineLine>,
    pub bestmove: Option<Move>,
    pub depth: u32,
    pub nodes: u64,
    pub time_ms: u64,
    pub identity: EngineIdentity,
    /// 候选来自同一深度的完整一轮，首着互不重复，且与引擎最终着法一致。
    #[serde(default)]
    pub complete: bool,
    /// 候选不完整或不一致的说明。
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum SearchOutcome {
    Completed(SearchResult),
    /// 局面已终局（将死或困毙），无需搜索。
    Terminal(GameStatus),
    Cancelled,
    Failed(String),
    /// 提交后引擎设置已改变，任务没有执行；调用方应按新设置重新提交。
    Stale(String),
}
