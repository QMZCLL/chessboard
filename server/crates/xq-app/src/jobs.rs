//! 后台任务的生命周期：讲解（含收件箱的分析）、分析、对比、局面决策、AI 讲解、追问，以及出题、导入导出、备份与云库查询。
//!
//! 每个任务有统一的标识与状态：
//! - `job_id`：一次实际执行；`operation_id`：用户的一次操作（重试保留同一个操作编号，`attempt` 加一）；
//! - `key`：任务针对的对象（如“explain:会话:节点”）。同一个对象开始新的任务时，之前还没有结束的任务记为“被取代”；
//! - 状态：排队 → 进行中 → 成功 / 失败 / 取消 / 被取代 / 中断。结束状态不再改变：之后到达的结果不改变记录
//!   （各任务写入数据前另有自己的检查，见 `pipeline::commit` 等，晚到的结果不会写入）。
//!
//! 退出时仍未结束的任务记为“中断”；上次运行留下的、在数据中仍标为进行中的任务在启动时记为中断（见 `service::start`）。
//! 登记只在内存中，最近结束的任务保留 `HISTORY` 条，用于诊断与测试。
//!
//! 各类任务在关闭会话、退出、重复发起、重试、晚到与重新启动时的处理见 `docs/data-guarantees.md`。

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;
use std::time::Instant;

use serde::Serialize;

/// 保留的已结束任务条数。
const HISTORY: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// 一着的讲解（引擎搜索与讲解生成）。
    Explain,
    /// 局面分析面板的搜索。
    Analyze,
    /// 试走对比。
    Compare,
    /// 复习中局面决策的比较。
    Decision,
    /// 一着的 AI 讲解。
    Coach,
    /// 教练追问。
    Ask,
    /// 出题与重新出题。
    Exercises,
    /// 导入学习档案。
    ImportArchive,
    /// 导入棋谱、FEN 或单个会话。
    ImportStudy,
    /// 导出单个会话或学习档案。
    Export,
    /// 备份学习档案。
    Backup,
    /// 云库查询。
    Cloud,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    /// 同一对象开始了新的任务，这次的结果不再使用。
    Superseded,
    /// 应用退出（或上次运行退出）时还没有结束。
    Interrupted,
}

impl JobState {
    pub fn is_final(self) -> bool { !matches!(self, JobState::Queued | JobState::Running) }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct JobRecord {
    pub job_id: String,
    pub operation_id: String,
    pub kind: JobKind,
    pub key: String,
    pub study_id: Option<String>,
    /// 任务开始时会话的棋谱版本。
    #[ts(type = "number | null")]
    pub subject_revision: Option<u64>,
    /// 同一操作的第几次执行（从 1 开始）。
    pub attempt: u32,
    pub state: JobState,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub finished_at: Option<i64>,
    /// 失败、取消、取代或中断的原因。
    pub detail: Option<String>,
    /// 是否写入了数据（成功的任务才可能写入）。
    pub committed: bool,
}

/// 开始一个任务时的说明。
pub struct NewJob {
    pub job_id: String,
    pub operation_id: Option<String>,
    pub kind: JobKind,
    pub key: String,
    pub study_id: Option<String>,
    pub subject_revision: Option<u64>,
    /// 为 false 时同一对象上可以同时有几个任务（如两次导出），开始时不取代之前的任务。
    pub supersedes: bool,
}

impl NewJob {
    pub fn new(kind: JobKind, job_id: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            job_id: job_id.into(),
            operation_id: None,
            kind,
            key: key.into(),
            study_id: None,
            subject_revision: None,
            supersedes: true,
        }
    }

    /// 同一对象上的几个任务可以同时进行，互不取代。
    pub fn concurrent(mut self) -> Self {
        self.supersedes = false;
        self
    }

    pub fn operation(mut self, id: impl Into<String>) -> Self {
        self.operation_id = Some(id.into());
        self
    }

    pub fn study(mut self, id: &str, revision: Option<u64>) -> Self {
        self.study_id = Some(id.to_string());
        self.subject_revision = revision;
        self
    }
}

/// 任务的结束方式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Succeeded { committed: bool },
    Failed(String),
    Cancelled(String),
    Superseded(String),
    Interrupted(String),
}

/// 已登记的任务：进行中的与最近结束的。
#[derive(Clone, Debug, Default, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct JobSnapshot {
    pub active: Vec<JobRecord>,
    /// 最近结束的任务，较早的在前。
    pub finished: Vec<JobRecord>,
    /// 任务已经结束之后才到达、因而没有改变记录的结束通知次数。
    pub late: u32,
}

#[derive(Default)]
struct State {
    active: BTreeMap<String, JobRecord>,
    finished: VecDeque<JobRecord>,
    /// 每个操作已经开始的执行次数。
    attempts: BTreeMap<String, u32>,
    late: u32,
}

#[derive(Default)]
pub struct JobRegistry {
    state: Mutex<State>,
    changed: Condvar,
    seq: std::sync::atomic::AtomicU64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl JobRegistry {
    fn lock(&self) -> MutexGuard<'_, State> { self.state.lock().unwrap_or_else(|p| p.into_inner()) }

    /// 新的任务编号（这次运行中不重复）。
    pub fn next_id(&self, kind: JobKind) -> String {
        format!("{}-{}", kind_name(kind), self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1)
    }

    /// 登记一个排队中的任务；同一对象之前还没有结束的任务记为被取代。返回任务编号。
    pub fn begin(&self, spec: NewJob) -> String {
        let mut st = self.lock();
        let earlier: Vec<String> = if spec.supersedes {
            st.active.values().filter(|r| r.kind == spec.kind && r.key == spec.key).map(|r| r.job_id.clone()).collect()
        } else {
            Vec::new()
        };
        for id in earlier {
            finish_locked(&mut st, &id, Outcome::Superseded(format!("被新的任务 {} 取代", spec.job_id)));
        }
        let operation_id = spec.operation_id.unwrap_or_else(|| spec.job_id.clone());
        let attempt = {
            let n = st.attempts.entry(operation_id.clone()).or_insert(0);
            *n += 1;
            *n
        };
        // 操作计数只为最近的任务保留
        if st.attempts.len() > HISTORY * 4 {
            let keep: std::collections::HashSet<String> =
                st.active.values().chain(st.finished.iter()).map(|r| r.operation_id.clone()).collect();
            st.attempts.retain(|k, _| keep.contains(k));
        }
        let record = JobRecord {
            job_id: spec.job_id.clone(),
            operation_id,
            kind: spec.kind,
            key: spec.key,
            study_id: spec.study_id,
            subject_revision: spec.subject_revision,
            attempt,
            state: JobState::Queued,
            created_at: now_ms(),
            finished_at: None,
            detail: None,
            committed: false,
        };
        if let Some(old) = st.active.insert(spec.job_id.clone(), record) {
            // 同一个编号重复登记：之前的那次不会再有结果
            let mut old = old;
            old.state = JobState::Superseded;
            old.detail = Some("同一任务编号重新登记".into());
            old.finished_at = Some(now_ms());
            push_finished(&mut st, old);
        }
        drop(st);
        self.changed.notify_all();
        spec.job_id
    }

    /// 开始执行（只改变排队中的任务）。
    pub fn running(&self, job_id: &str) {
        let mut st = self.lock();
        if let Some(r) = st.active.get_mut(job_id)
            && r.state == JobState::Queued
        {
            r.state = JobState::Running;
        }
    }

    /// 结束一个任务；任务已经结束（或不存在）时不改变记录，返回 false。
    pub fn finish(&self, job_id: &str, outcome: Outcome) -> bool {
        let done = finish_locked(&mut self.lock(), job_id, outcome);
        self.changed.notify_all();
        done
    }

    /// 结束某个对象上还没有结束的任务（如删除了着法或会话），返回结束的个数。
    pub fn finish_key(&self, kind: JobKind, key: &str, outcome: Outcome) -> usize {
        let mut st = self.lock();
        let ids: Vec<String> = st.active.values().filter(|r| r.kind == kind && r.key == key).map(|r| r.job_id.clone()).collect();
        let n = ids.iter().filter(|id| finish_locked(&mut st, id, outcome.clone())).count();
        drop(st);
        self.changed.notify_all();
        n
    }

    /// 结束一个会话上全部还没有结束的任务（删除会话时），返回结束的个数。
    pub fn finish_study(&self, study_id: &str, outcome: Outcome) -> usize {
        let mut st = self.lock();
        let ids: Vec<String> =
            st.active.values().filter(|r| r.study_id.as_deref() == Some(study_id)).map(|r| r.job_id.clone()).collect();
        let n = ids.iter().filter(|id| finish_locked(&mut st, id, outcome.clone())).count();
        drop(st);
        self.changed.notify_all();
        n
    }

    /// 是否仍在进行（没有结束、没有被取代或取消）。
    pub fn is_active(&self, job_id: &str) -> bool { self.lock().active.contains_key(job_id) }

    /// 退出时把还没有结束的任务记为中断，返回个数。
    pub fn interrupt_all(&self, reason: &str) -> usize {
        let mut st = self.lock();
        let ids: Vec<String> = st.active.keys().cloned().collect();
        for id in &ids {
            finish_locked(&mut st, id, Outcome::Interrupted(reason.to_string()));
        }
        drop(st);
        self.changed.notify_all();
        ids.len()
    }

    /// 记下一个上次运行留下、启动时才发现中断的任务。
    pub fn record_interrupted(&self, kind: JobKind, key: String, study_id: Option<String>, detail: &str) {
        let id = format!("{}:interrupted:{key}", kind_name(kind));
        let mut st = self.lock();
        let record = JobRecord {
            job_id: id.clone(),
            operation_id: id,
            kind,
            key,
            study_id,
            subject_revision: None,
            attempt: 1,
            state: JobState::Interrupted,
            created_at: now_ms(),
            finished_at: Some(now_ms()),
            detail: Some(detail.to_string()),
            committed: false,
        };
        push_finished(&mut st, record);
    }

    pub fn snapshot(&self) -> JobSnapshot {
        let st = self.lock();
        JobSnapshot {
            active: st.active.values().cloned().collect(),
            finished: st.finished.iter().cloned().collect(),
            late: st.late,
        }
    }

    /// 等到没有进行中的任务（或超时），返回是否等到。
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut st = self.lock();
        while !st.active.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            st = self.changed.wait_timeout(st, left).unwrap_or_else(|p| p.into_inner()).0;
        }
        true
    }
}

fn kind_name(kind: JobKind) -> String { serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default() }

fn push_finished(st: &mut State, record: JobRecord) {
    st.finished.push_back(record);
    while st.finished.len() > HISTORY {
        st.finished.pop_front();
    }
}

fn finish_locked(st: &mut State, job_id: &str, outcome: Outcome) -> bool {
    let Some(mut r) = st.active.remove(job_id) else {
        st.late += 1;
        return false;
    };
    let (state, detail, committed) = match outcome {
        Outcome::Succeeded { committed } => (JobState::Succeeded, None, committed),
        Outcome::Failed(d) => (JobState::Failed, Some(d), false),
        Outcome::Cancelled(d) => (JobState::Cancelled, Some(d), false),
        Outcome::Superseded(d) => (JobState::Superseded, Some(d), false),
        Outcome::Interrupted(d) => (JobState::Interrupted, Some(d), false),
    };
    r.state = state;
    r.detail = detail;
    r.committed = committed;
    r.finished_at = Some(now_ms());
    push_finished(st, r);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_job_on_the_same_key_supersedes_the_running_one_and_late_results_change_nothing() {
        let jobs = JobRegistry::default();
        let a = jobs.begin(NewJob::new(JobKind::Explain, "explain:1", "explain:s:3").study("s", Some(4)));
        jobs.running(&a);
        let b = jobs.begin(NewJob::new(JobKind::Explain, "explain:2", "explain:s:3").study("s", Some(4)));
        assert!(!jobs.finish(&a, Outcome::Succeeded { committed: true }), "被取代的任务的结果不改变记录");
        assert!(jobs.finish(&b, Outcome::Succeeded { committed: true }));
        let snap = jobs.snapshot();
        assert!(snap.active.is_empty());
        assert_eq!(snap.late, 1);
        let states: Vec<(String, JobState, bool)> = snap.finished.iter().map(|r| (r.job_id.clone(), r.state, r.committed)).collect();
        assert_eq!(states, vec![("explain:1".into(), JobState::Superseded, false), ("explain:2".into(), JobState::Succeeded, true)]);
    }

    #[test]
    fn retries_of_one_operation_count_attempts() {
        let jobs = JobRegistry::default();
        let a = jobs.begin(NewJob::new(JobKind::Coach, "coach:1", "coach:s:1").operation("op"));
        jobs.finish(&a, Outcome::Failed("服务端错误".into()));
        let b = jobs.begin(NewJob::new(JobKind::Coach, "coach:2", "coach:s:1").operation("op"));
        jobs.finish(&b, Outcome::Succeeded { committed: true });
        let attempts: Vec<u32> = jobs.snapshot().finished.iter().map(|r| r.attempt).collect();
        assert_eq!(attempts, vec![1, 2]);
    }

    #[test]
    fn exit_interrupts_whatever_is_left() {
        let jobs = JobRegistry::default();
        jobs.begin(NewJob::new(JobKind::Analyze, "analyze:r1", "analyze:r1"));
        jobs.begin(NewJob::new(JobKind::Coach, "coach:9", "coach:s:2").study("s", None));
        assert_eq!(jobs.finish_study("s", Outcome::Cancelled("会话已删除".into())), 1);
        assert_eq!(jobs.interrupt_all("应用退出"), 1);
        assert!(jobs.wait_idle(Duration::from_millis(10)));
        assert!(jobs.snapshot().finished.iter().all(|r| r.state.is_final()));
    }
}
