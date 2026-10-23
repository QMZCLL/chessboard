//! AI 讲解任务。
//!
//! - 网络请求在独立的工作线程中进行，不占用讲解线程；并发数、超时与每天的调用次数受设置限制，
//!   网络错误与限流重试一次，输出没有通过检查时让模型按检查结果修正一次。
//! - 登记请求、等待引擎分析、提交结果都在讲解线程上执行，与逐着讲解的写入依次进行。
//!   提交前确认任务仍是该节点最新的请求、证据版本与模型设置都没有改变，旧结果不会覆盖新结果。
//! - 引擎分析尚未完成时先等待，讲解记录写入后再生成，避免用很快就会过期的证据调用模型。

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use tracing::debug;
use tracing::warn;
use xq_core::study::NodeId;
use xq_core::study::Study;

use super::CoachExplanation;
use super::CoachRecord;
use super::CoachResult;
use super::CoachState;
use super::CoachStatus;
use super::CoachView;
use super::credentials;
use super::evidence;
use super::evidence::EvidenceBundle;
use super::prompt;
use super::provider;
use super::provider::Message;
use super::provider::ProviderSettings;
use super::validate;
use crate::api::AppStatus;
use crate::api::CoachAvailability;
use crate::api::CoachCredentialRequest;
use crate::api::CoachExplainRequest;
use crate::api::NodeRef;
use crate::config::Audience;
use crate::config::Detail;
use crate::engine::Priority;
use crate::events;
use crate::records::Budget;
use crate::records::ExplanationRecord;
use crate::service::ApiResult;
use crate::service::AppService;
use crate::service::Inner;
use crate::service::now_ms;

/// 工作线程数；实际并发受设置中的“并发请求数”限制。
const WORKERS: usize = 4;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> { m.lock().unwrap_or_else(|p| p.into_inner()) }

/// UTC 日序号，用于每日调用次数。
fn today() -> i64 { now_ms().div_euclid(86_400_000) }

#[derive(Clone, Debug)]
pub(crate) struct CoachRequest {
    pub study_id: String,
    pub node_id: NodeId,
    pub audience: Audience,
    pub detail: Detail,
    /// 忽略已有结果与缓存，重新生成。
    pub refresh: bool,
}

pub(crate) struct CoachJob {
    request: CoachRequest,
    generation: u64,
    /// 派发时的设置版本；设置改变后，尚未发出的请求不再发送。
    config_rev: u64,
    bundle: EvidenceBundle,
    cache_key: String,
    settings: ProviderSettings,
}

/// 讲解线程与工作线程共用的运行状态。
pub(crate) struct CoachRuntime {
    jobs: Mutex<Sender<CoachJob>>,
    /// 进行中的请求：(会话, 节点) → (代次, 缓存键)。
    inflight: Mutex<HashMap<(String, NodeId), (u64, String)>>,
    /// 等待引擎分析完成的请求。
    waiting: Mutex<HashMap<(String, NodeId), CoachRequest>>,
    generation: AtomicU64,
    /// 模型服务设置（启用、接口、模型、凭据）的版本。
    config_rev: AtomicU64,
    active: Mutex<usize>,
    slot_free: Condvar,
}

impl CoachRuntime {
    pub(crate) fn new(jobs: Sender<CoachJob>) -> Self {
        Self {
            jobs: Mutex::new(jobs),
            inflight: Mutex::new(HashMap::new()),
            waiting: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            config_rev: AtomicU64::new(0),
            active: Mutex::new(0),
            slot_free: Condvar::new(),
        }
    }
}

/// 缓存键：证据版本（含规则事实生成版本与证据包结构版本）、提示词与输出结构版本、模型配置、
/// 学习水平、详细程度与任务类型都相同时，结果可以直接复用。
pub(crate) fn cache_key(bundle: &EvidenceBundle, settings: &ProviderSettings, audience: Audience, detail: Detail) -> String {
    let text = format!(
        "explain_move|{}|{}|{}|{}|{audience:?}|{detail:?}",
        bundle.revision,
        prompt::PROMPT_VERSION,
        prompt::OUTPUT_VERSION,
        settings.revision()
    );
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn spawn_workers(weak: Weak<Inner>, rx: Receiver<CoachJob>) -> Result<(), String> {
    let rx = Arc::new(Mutex::new(rx));
    for i in 0..WORKERS {
        let (weak, rx) = (weak.clone(), rx.clone());
        thread::Builder::new()
            .name(format!("coach-{i}"))
            .spawn(move || worker(weak, rx))
            .map_err(|e| format!("无法启动 AI 讲解线程：{e}"))?;
    }
    Ok(())
}

fn worker(weak: Weak<Inner>, rx: Arc<Mutex<Receiver<CoachJob>>>) {
    loop {
        let job = match lock(&rx).recv() {
            Ok(job) => job,
            Err(_) => return,
        };
        let Some(inner) = weak.upgrade() else { return };
        if !inner.coach_current(&job) {
            continue;
        }
        inner.coach_acquire();
        let (key, generation) = ((job.request.study_id.clone(), job.request.node_id), job.generation);
        inner.on_explain_thread(move |inner| inner.coach_mark_running(key, generation));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&inner, &job)))
            .unwrap_or_else(|_| Err("生成 AI 讲解时出错".to_string()));
        inner.coach_release();
        inner.on_explain_thread(move |inner| inner.coach_commit(job, outcome));
    }
}

/// 请求模型并检查输出；没有通过检查时，带着检查结果让模型修正一次。
fn run_job(inner: &Inner, job: &CoachJob) -> Result<CoachExplanation, String> {
    let ids: Vec<String> = job.bundle.pack.claims.iter().map(|c| c.id.clone()).collect();
    let schema = prompt::output_schema(&ids);
    let mut messages = vec![
        Message::system(prompt::system_prompt()),
        Message::user(prompt::user_message(&job.bundle.pack, job.request.audience, job.request.detail)),
    ];
    let mut errors = Vec::new();
    for attempt in 0..2 {
        let content = inner.coach_call(job, &messages, &schema)?;
        match validate::validate(&content, &job.bundle, job.request.detail) {
            Ok(out) => return Ok(out),
            Err(found) => {
                debug!("AI 讲解第 {} 次输出没有通过检查：{found:?}", attempt + 1);
                messages.push(Message::assistant(content));
                messages.push(Message::user(prompt::repair_message(&found)));
                errors = found;
            }
        }
    }
    Err(format!("AI 讲解没有通过检查：{}", errors.join("；")))
}

impl Inner {
    /// 当前设置下的模型服务；不可用时给出原因。
    pub(crate) fn coach_settings(&self) -> Result<ProviderSettings, String> {
        let cfg = self.config_snapshot().coach;
        if !cfg.enabled {
            return Err("AI 讲解未启用，可在设置中开启".into());
        }
        if cfg.model.is_empty() {
            return Err("请在设置中填写模型名称".into());
        }
        let key = credentials::resolve(&self.paths.config_dir, cfg.credential_ref.as_deref())?;
        if cfg.provider.needs_key() && key.is_none() {
            return Err(format!("请在设置中填写 {} 的密钥", cfg.provider.label()));
        }
        Ok(ProviderSettings::from_config(&cfg, key))
    }

    pub(crate) fn coach_availability(&self) -> CoachAvailability {
        let cfg = self.config_snapshot().coach;
        let key_configured =
            credentials::resolve(&self.paths.config_dir, cfg.credential_ref.as_deref()).ok().flatten().is_some();
        let settings = self.coach_settings();
        CoachAvailability {
            enabled: cfg.enabled,
            ready: settings.is_ok(),
            key_configured,
            reason: settings.err(),
            model_label: format!("{} · {}", cfg.provider.label(), cfg.model),
            used_today: self.store.coach_usage(today()).unwrap_or(0),
            daily_budget: cfg.daily_request_budget,
        }
    }

    fn coach_bundle(&self, study: &Study, node_id: NodeId) -> Result<EvidenceBundle, String> {
        let record = self
            .store
            .load_explanation(&study.id, node_id)
            .map_err(|e| e.to_string())?
            .ok_or("这一着还没有讲解")?;
        evidence::build(study, node_id, &record)
    }

    /// 界面看到的状态：结果是否对应当前的证据、模型与讲解设置。
    pub(crate) fn coach_view(&self, study_id: &str, node_id: NodeId) -> CoachView {
        let record = self.store.load_coach(study_id, node_id).ok().flatten();
        let settings = self.coach_settings();
        let cfg = self.config_snapshot().coach;
        let current = settings.as_ref().ok().and_then(|s| {
            let study = self.with_study(study_id, |st| Ok(st.clone())).ok()?;
            let bundle = self.coach_bundle(&study, node_id).ok()?;
            Some((bundle.revision.clone(), cache_key(&bundle, s, cfg.audience, cfg.detail)))
        });
        let is_current = |res: &CoachResult| current.as_ref().is_some_and(|(_, key)| *key == res.cache_key);
        let (status, reason) = match &record {
            Some(r) if r.state == CoachState::Queued => {
                (CoachStatus::Queued, r.waiting_evidence.then(|| "等待引擎分析完成后生成".to_string()))
            }
            Some(r) if r.state == CoachState::Running => (CoachStatus::Running, None),
            Some(r) if r.result.as_ref().is_some_and(is_current) => {
                let reason = r.error.as_ref().map(|e| match r.state {
                    CoachState::Failed => format!("最近一次重新生成失败：{e}"),
                    _ => e.clone(),
                });
                (CoachStatus::Ready, reason)
            }
            Some(r) if r.state == CoachState::Failed => (CoachStatus::Failed, r.error.clone()),
            Some(CoachRecord { result: Some(res), .. }) => {
                let reason = match (&settings, &current) {
                    (Err(e), _) => format!("AI 讲解当前不可用：{e}"),
                    (_, Some((revision, _))) if *revision != res.evidence_revision => {
                        "讲解依据已更新（引擎分析或讲解规则有变化），可以重新生成".to_string()
                    }
                    (_, Some(_)) => "模型或讲解设置已改变，可以重新生成".to_string(),
                    (_, None) => "讲解依据或设置已改变，可以重新生成".to_string(),
                };
                (CoachStatus::Stale, Some(reason))
            }
            _ => match settings {
                Err(e) => (CoachStatus::Disabled, Some(e)),
                // 例如上次生成在应用退出时中断
                Ok(_) => (CoachStatus::None, record.as_ref().and_then(|r| r.error.clone())),
            },
        };
        CoachView { study_id: study_id.to_string(), node_id, status, reason, record }
    }

    fn emit_coach(&self, study_id: &str, node_id: NodeId) {
        events::emit(self.events.as_ref(), events::COACH_UPDATED, &self.coach_view(study_id, node_id));
    }

    fn update_coach(&self, study_id: &str, node_id: NodeId, f: impl FnOnce(&mut CoachRecord)) {
        if !self.node_exists(study_id, node_id) {
            return;
        }
        let mut record =
            self.store.load_coach(study_id, node_id).ok().flatten().unwrap_or_else(|| CoachRecord::new(study_id, node_id, now_ms()));
        f(&mut record);
        record.updated_at = now_ms();
        if let Err(e) = self.store.save_coach(&record) {
            warn!("保存 AI 讲解失败：{e}");
            return;
        }
        self.emit_coach(study_id, node_id);
    }

    /// 任务仍是节点最新的请求，且模型服务设置没有改变。
    fn coach_current(&self, job: &CoachJob) -> bool {
        let key = (job.request.study_id.clone(), job.request.node_id);
        lock(&self.coach.inflight).get(&key).map(|(g, _)| *g) == Some(job.generation)
            && self.coach.config_rev.load(Ordering::SeqCst) == job.config_rev
    }

    /// 模型服务设置或密钥改变：使尚未发出的请求失效，并在讲解线程上按新设置重新安排。
    pub(crate) fn coach_config_changed(&self) {
        self.coach.config_rev.fetch_add(1, Ordering::SeqCst);
        self.on_explain_thread(|inner| inner.coach_settings_changed());
    }

    fn coach_settings_changed(&self) {
        let usable = self.coach_settings().is_ok();
        let cfg = self.config_snapshot().coach;
        // 已经发出的请求返回后丢弃；排队中的请求不再发送
        let mut affected: Vec<(String, NodeId)> = lock(&self.coach.inflight).drain().map(|(k, _)| k).collect();
        if !usable {
            affected.extend(lock(&self.coach.waiting).drain().map(|(k, _)| k));
        }
        for (sid, nid) in affected {
            if usable {
                // 按新设置重新安排；结果相同时直接使用缓存
                let req = CoachRequest { study_id: sid, node_id: nid, audience: cfg.audience, detail: cfg.detail, refresh: false };
                self.coach_request(req);
            } else {
                self.update_coach(&sid, nid, |c| {
                    if matches!(c.state, CoachState::Queued | CoachState::Running) {
                        c.state = CoachState::Idle;
                    }
                    c.waiting_evidence = false;
                });
            }
        }
    }

    fn coach_acquire(&self) {
        let mut active = lock(&self.coach.active);
        while *active >= self.config_snapshot().coach.max_parallel_requests.max(1) as usize {
            active = self.coach.slot_free.wait(active).unwrap_or_else(|p| p.into_inner());
        }
        *active += 1;
    }

    fn coach_release(&self) {
        *lock(&self.coach.active) -= 1;
        self.coach.slot_free.notify_all();
    }

    /// 发出一次请求（计入每日调用次数）；网络错误与限流稍后重试一次。
    fn coach_call(&self, job: &CoachJob, messages: &[Message], schema: &Value) -> Result<String, String> {
        let budget = self.config_snapshot().coach.daily_request_budget;
        let mut last = String::new();
        for attempt in 0..2 {
            if !self.coach_current(job) {
                return Err("请求已取消、已被更新的请求取代，或模型服务设置已改变".into());
            }
            // 取得并发名额后、扣减调用次数与发送前再确认一次：关闭功能、清除密钥或更换服务后不再发送
            match self.store.take_coach_budget(today(), budget) {
                Ok(true) => {}
                Ok(false) => return Err(format!("今天的 AI 讲解调用次数已达上限（{budget} 次），可在设置中调整")),
                Err(e) => return Err(e.to_string()),
            }
            match provider::complete(&job.settings, messages, schema) {
                Ok(text) => return Ok(text),
                Err(e) if e.retryable() && attempt == 0 => {
                    warn!("AI 讲解请求失败，稍后重试：{e}");
                    last = e.to_string();
                    thread::sleep(Duration::from_millis(800));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(last)
    }

    /// 登记一次 AI 讲解请求（在讲解线程上执行）。
    pub(crate) fn coach_request(&self, req: CoachRequest) {
        let (sid, nid) = (req.study_id.clone(), req.node_id);
        if self.coach_settings().is_err() {
            // 不可用：结束可能残留的排队状态，界面据此显示原因
            if self.store.load_coach(&sid, nid).ok().flatten().is_some() {
                self.update_coach(&sid, nid, |r| {
                    if matches!(r.state, CoachState::Queued | CoachState::Running) {
                        r.state = CoachState::Idle;
                        r.waiting_evidence = false;
                    }
                });
            } else {
                self.emit_coach(&sid, nid);
            }
            return;
        }
        let Ok(study) = self.with_study(&sid, |s| Ok(s.clone())) else { return };
        if !study.nodes.get(&nid).is_some_and(|n| n.is_move()) {
            return;
        }
        match self.store.load_explanation(&sid, nid).ok().flatten() {
            Some(r) if r.explanation.is_some() && !r.pending => {
                let settings = match self.coach_settings() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                self.coach_dispatch(req, &study, &r, settings);
            }
            Some(r) if r.explanation.is_none() && !r.pending && r.error.is_some() => {
                let error = format!("这一着没有可用的讲解：{}", r.error.unwrap_or_default());
                self.update_coach(&sid, nid, |c| {
                    c.state = CoachState::Failed;
                    c.waiting_evidence = false;
                    c.error = Some(error);
                });
            }
            _ => {
                // 引擎分析还在进行：讲解记录完成后再生成
                let key = (sid.clone(), nid);
                lock(&self.coach.inflight).remove(&key);
                lock(&self.coach.waiting).insert(key, req);
                self.update_coach(&sid, nid, |c| {
                    c.state = CoachState::Queued;
                    c.waiting_evidence = true;
                    c.error = None;
                });
            }
        }
    }

    fn coach_dispatch(&self, req: CoachRequest, study: &Study, record: &ExplanationRecord, settings: ProviderSettings) {
        let key = (req.study_id.clone(), req.node_id);
        let (sid, nid) = key.clone();
        lock(&self.coach.waiting).remove(&key);
        let bundle = match evidence::build(study, nid, record) {
            Ok(b) => b,
            Err(e) => {
                lock(&self.coach.inflight).remove(&key);
                self.update_coach(&sid, nid, |c| {
                    c.state = CoachState::Failed;
                    c.waiting_evidence = false;
                    c.error = Some(e);
                });
                return;
            }
        };
        let cache_key = cache_key(&bundle, &settings, req.audience, req.detail);
        // 同样的请求已在进行
        if lock(&self.coach.inflight).get(&key).is_some_and(|(_, k)| *k == cache_key) {
            self.emit_coach(&sid, nid);
            return;
        }
        if !req.refresh {
            let existing = self.store.load_coach(&sid, nid).ok().flatten();
            let cached = match existing.as_ref().and_then(|r| r.result.as_ref()) {
                Some(res) if res.cache_key == cache_key => None,
                _ => self.store.load_coach_cache(&sid, nid, &cache_key).ok().flatten(),
            };
            let has_current = existing.as_ref().and_then(|r| r.result.as_ref()).is_some_and(|r| r.cache_key == cache_key);
            if has_current || cached.is_some() {
                // 已有对应当前证据与设置的结果：不再调用模型
                lock(&self.coach.inflight).remove(&key);
                self.update_coach(&sid, nid, |c| {
                    if let Some(result) = cached {
                        c.result = Some(result);
                        c.error = None;
                        c.state = CoachState::Ready;
                    } else if matches!(c.state, CoachState::Queued | CoachState::Running) {
                        c.state = CoachState::Ready;
                    }
                    c.waiting_evidence = false;
                });
                return;
            }
        }
        let generation = self.coach.generation.fetch_add(1, Ordering::SeqCst) + 1;
        lock(&self.coach.inflight).insert(key.clone(), (generation, cache_key.clone()));
        self.update_coach(&sid, nid, |c| {
            c.state = CoachState::Queued;
            c.waiting_evidence = false;
            c.generation = generation;
            c.error = None;
        });
        let config_rev = self.coach.config_rev.load(Ordering::SeqCst);
        let job = CoachJob { request: req, generation, config_rev, bundle, cache_key, settings };
        if lock(&self.coach.jobs).send(job).is_err() {
            lock(&self.coach.inflight).remove(&key);
            self.update_coach(&sid, nid, |c| {
                c.state = CoachState::Failed;
                c.error = Some("AI 讲解线程已停止".into());
            });
        }
    }

    /// 逐着讲解记录写入后调用（讲解线程）：等待证据的请求开始生成，已有的 AI 讲解刷新状态。
    pub(crate) fn coach_on_explanation(&self, record: &ExplanationRecord) {
        let key = (record.study_id.clone(), record.node_id);
        if !record.pending {
            let waiting = lock(&self.coach.waiting).remove(&key);
            if let Some(req) = waiting {
                self.coach_request(req);
                return;
            }
        }
        if self.store.load_coach(&record.study_id, record.node_id).ok().flatten().is_some() {
            self.emit_coach(&record.study_id, record.node_id);
        }
    }

    fn coach_mark_running(&self, key: (String, NodeId), generation: u64) {
        if lock(&self.coach.inflight).get(&key).map(|(g, _)| *g) == Some(generation) {
            self.update_coach(&key.0, key.1, |c| c.state = CoachState::Running);
        }
    }

    /// 提交结果（讲解线程）：任务仍是最新请求，且证据版本与模型设置都没有改变时才写入。
    fn coach_commit(&self, job: CoachJob, outcome: Result<CoachExplanation, String>) {
        let key = (job.request.study_id.clone(), job.request.node_id);
        {
            let mut inflight = lock(&self.coach.inflight);
            if inflight.get(&key).map(|(g, _)| *g) != Some(job.generation) {
                debug!("节点 {} 的 AI 讲解已被取消或被更新的请求取代，丢弃结果", job.request.node_id);
                return;
            }
            inflight.remove(&key);
        }
        let (sid, nid) = key;
        let Ok(study) = self.with_study(&sid, |s| Ok(s.clone())) else { return };
        if !study.nodes.contains_key(&nid) {
            return;
        }
        let evidence_same = self
            .store
            .load_explanation(&sid, nid)
            .ok()
            .flatten()
            .filter(|r| !r.pending)
            .and_then(|r| evidence::build(&study, nid, &r).ok())
            .is_some_and(|b| b.revision == job.bundle.revision);
        let settings_same = self.coach_settings().is_ok_and(|s| s.revision() == job.settings.revision())
            && self.coach.config_rev.load(Ordering::SeqCst) == job.config_rev;
        if !(evidence_same && settings_same) {
            debug!("节点 {nid} 的讲解依据或模型设置在生成期间改变，按当前状态重新安排");
            self.coach_request(CoachRequest { refresh: false, ..job.request });
            return;
        }
        match outcome {
            Ok(output) => {
                let result = result_of(&job, output);
                if let Err(e) = self.store.save_coach_cache(&sid, nid, &result) {
                    warn!("保存 AI 讲解缓存失败：{e}");
                }
                self.update_coach(&sid, nid, |c| {
                    c.state = CoachState::Ready;
                    c.waiting_evidence = false;
                    c.result = Some(result);
                    c.error = None;
                });
            }
            Err(e) => self.update_coach(&sid, nid, |c| {
                c.state = CoachState::Failed;
                c.waiting_evidence = false;
                c.error = Some(e);
            }),
        }
    }

    /// 取消排队的请求；已经发出的请求在返回后丢弃结果。
    fn coach_cancel(&self, study_id: &str, node_id: NodeId) {
        let key = (study_id.to_string(), node_id);
        lock(&self.coach.waiting).remove(&key);
        lock(&self.coach.inflight).remove(&key);
        if self.store.load_coach(study_id, node_id).ok().flatten().is_some() {
            self.update_coach(study_id, node_id, |c| {
                if matches!(c.state, CoachState::Queued | CoachState::Running) {
                    c.state = CoachState::Idle;
                }
                c.waiting_evidence = false;
            });
        }
    }

    /// 会话或节点删除后，丢弃对应的等待与进行中的请求。
    pub(crate) fn coach_forget(&self, study_id: &str, nodes: Option<&[NodeId]>) {
        let hit = |k: &(String, NodeId)| k.0 == study_id && nodes.is_none_or(|n| n.contains(&k.1));
        lock(&self.coach.waiting).retain(|k, _| !hit(k));
        lock(&self.coach.inflight).retain(|k, _| !hit(k));
    }
}

/// 整理成保存的结果：附上引用到的结论与证据（含后续操作对应的回放证据）。
fn result_of(job: &CoachJob, output: CoachExplanation) -> CoachResult {
    let cited: HashSet<&String> = output
        .sections
        .iter()
        .flat_map(|s| s.claim_ids.iter())
        .chain(output.summary.claim_ids.iter())
        .chain(output.lesson.claim_ids.iter())
        .collect();
    let claims: Vec<_> = job.bundle.pack.claims.iter().filter(|c| cited.contains(&c.id)).cloned().collect();
    let mut evidence_ids: HashSet<String> = claims.iter().flat_map(|c| c.evidence_ids.iter().cloned()).collect();
    evidence_ids.extend(output.followups.iter().filter_map(|f| f.evidence_id.clone()));
    let evidence = job.bundle.evidence.iter().filter(|e| evidence_ids.contains(&e.id)).cloned().collect();
    CoachResult {
        output,
        claims,
        evidence,
        evidence_revision: job.bundle.revision.clone(),
        cache_key: job.cache_key.clone(),
        model_label: job.settings.label(),
        audience: job.request.audience,
        detail: job.request.detail,
        created_at: now_ms(),
    }
}

impl AppService {
    /// 生成某一着的 AI 讲解。已有对应当前证据与设置的结果时直接返回，不再调用模型；
    /// 引擎分析还在进行时先等待。结果通过 `coach_updated` 事件推送。
    pub fn coach_explain(&self, req: CoachExplainRequest) -> ApiResult<CoachView> {
        let study = self.inner.with_study(&req.study_id, |s| Ok(s.clone()))?;
        if !study.node(req.node_id)?.is_move() {
            return Err("起始局面和缺口没有逐着讲解，不能生成 AI 讲解".into());
        }
        let cfg = self.inner.config_snapshot().coach;
        let request = CoachRequest {
            study_id: req.study_id.clone(),
            node_id: req.node_id,
            audience: req.audience.unwrap_or(cfg.audience),
            detail: req.detail.unwrap_or(cfg.detail),
            refresh: req.refresh,
        };
        // 先确保逐着讲解已排队；AI 讲解的登记排在它之后，届时已能看出引擎分析是否在进行
        self.inner.enqueue(&req.study_id, req.node_id, Budget::Quick, Priority::Interactive, true);
        self.inner.on_explain_thread(move |inner| inner.coach_request(request));
        Ok(self.inner.coach_view(&req.study_id, req.node_id))
    }

    pub fn coach_cancel(&self, r: NodeRef) -> ApiResult<CoachView> {
        self.inner.with_study(&r.study_id, |s| Ok(s.node(r.node_id).map(|_| ())?))?;
        let (sid, nid) = (r.study_id.clone(), r.node_id);
        self.inner.on_explain_thread(move |inner| inner.coach_cancel(&sid, nid));
        Ok(self.inner.coach_view(&r.study_id, r.node_id))
    }

    pub fn coach_view(&self, r: NodeRef) -> ApiResult<CoachView> {
        self.inner.with_study(&r.study_id, |s| Ok(s.node(r.node_id).map(|_| ())?))?;
        Ok(self.inner.coach_view(&r.study_id, r.node_id))
    }

    /// 保存或清除模型服务的密钥。密钥只写入后台凭据文件，配置中只记录引用。
    pub fn set_coach_credential(&self, req: CoachCredentialRequest) -> ApiResult<AppStatus> {
        let key = req.key.as_deref().map(str::trim).filter(|k| !k.is_empty());
        credentials::store(&self.inner.paths.config_dir, key)?;
        let mut cfg = self.inner.config_snapshot();
        let before = cfg.coach.credential_ref.clone();
        match key {
            Some(_) => cfg.coach.credential_ref = Some(credentials::STORE_REF.to_string()),
            None if cfg.coach.credential_ref.as_deref() == Some(credentials::STORE_REF) => cfg.coach.credential_ref = None,
            None => {}
        }
        let unchanged_ref = cfg.coach.credential_ref == before;
        let status = self.update_config(cfg)?;
        // 引用改变时 update_config 已经处理；引用不变而密钥本身换了，同样让排队中的请求按新密钥重新安排
        if unchanged_ref {
            self.inner.coach_config_changed();
        }
        Ok(status)
    }
}
