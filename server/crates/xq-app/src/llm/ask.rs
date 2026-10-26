//! 教练追问：围绕一组试走对比（同一比较起点的“我的选择”与“对照选择”）提出结构化的问题，
//! 由模型依据这组对比中的程序结论回答。
//!
//! - 证据只取已经得到的对比（规则事实与同一次搜索，见 `Inner::cached_comparison`），提问不会发起新的搜索；
//!   对比搜索还没有完成时不能提问。
//! - 模型只能引用证据中的结论编号，回答经过与 AI 讲解相同的硬结论检查（见 `validate::validate_answer`）；
//!   演示哪条变化由程序按问题类型决定，模型不提供。
//! - 回答连同引用的结论与演示一起保存，按比较起点、两步着法与问题类型归属：离开后再回到这组对比，
//!   仍显示原来的回答；对比依据或模型设置改变后标为过期，可以重新回答。
//! - 调用次数、并发、重试与修正都与 AI 讲解共用（见 `service`）。

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use tracing::debug;
use tracing::warn;
use xq_core::Move;
use xq_core::explain::Certainty;
use xq_core::explain::Claim;
use xq_core::explain::Evidence;
use xq_core::explain::EvidenceKind;
use xq_core::explain::LineBase;
use xq_core::explain::LineRef;
use xq_core::explain::compare::COMPARE_VERSION;
use xq_core::explain::compare::CompareSide;
use xq_core::study::NodeId;

use super::CoachState;
use super::CoachStatus;
use super::CoachText;
use super::evidence::AllowedClaim;
use super::evidence::fingerprint;
use super::evidence::weakest;
use super::prompt;
use super::provider::Message;
use super::provider::ProviderSettings;
use super::service::CoachTask;
use super::service::OUTPUT_ATTEMPTS;
use super::validate;
use crate::compare::CachedComparison;
use crate::compare::ReferenceKind;
use crate::config::Audience;
use crate::events;
use crate::service::ApiError;
use crate::service::ApiResult;
use crate::service::Inner;
use crate::service::now_ms;

/// 证据包结构版本；字段或生成方式改变时递增，旧的回答随之过期。
pub const ASK_PACK_VERSION: u32 = 1;
/// 证据包中最多列出的比较起点之前的着法数。
const HISTORY_LIMIT: usize = 30;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> { m.lock().unwrap_or_else(|p| p.into_inner()) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    /// 我的这一步起了什么作用。
    MyMove,
    /// 两步的差别在哪里。
    Difference,
    /// 走这一步之后，对方的回应需要怎样应对。
    Reply,
}

impl QuestionKind {
    pub const ALL: [QuestionKind; 3] = [QuestionKind::MyMove, QuestionKind::Difference, QuestionKind::Reply];

    fn slug(self) -> &'static str {
        match self {
            QuestionKind::MyMove => "my_move",
            QuestionKind::Difference => "difference",
            QuestionKind::Reply => "reply",
        }
    }

    /// 问题原文。
    pub fn text(self, mine: &str, reference: &str) -> String {
        match self {
            QuestionKind::MyMove => format!("我的选择{mine}起了什么作用？"),
            QuestionKind::Difference => format!("{mine}与{reference}的差别在哪里？"),
            QuestionKind::Reply => format!("走{mine}之后，对方的回应需要怎样应对？"),
        }
    }

    /// 回答必须引用其中一类结论；证据中没有这类结论时不能回答这个问题。
    pub fn required_groups(self) -> &'static [&'static str] {
        match self {
            QuestionKind::MyMove => &["mine"],
            QuestionKind::Difference => &["difference"],
            QuestionKind::Reply => &["mine_reply"],
        }
    }

    /// 证据不足以回答时的说明。
    fn missing(self) -> &'static str {
        match self {
            QuestionKind::MyMove => "这一步没有可以单独说明的直接作用，暂时不能回答这个问题",
            QuestionKind::Difference => "两步在规则事实上没有直接可比的差别，也还没有两步的搜索结果，暂时不能回答这个问题",
            QuestionKind::Reply => "对比中没有能合法回放的对方回应，暂时不能回答这个问题",
        }
    }
}

/// 一个问题：比较起点、两步着法与问题类型。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub study_id: String,
    /// 比较起点：两步棋都从这个节点的局面走出。
    pub node_id: NodeId,
    pub mine: Move,
    pub reference: Move,
    pub kind: QuestionKind,
}

impl Question {
    /// 保存的键：以“会话/比较起点/”开头，删除会话或节点时随讲解一起删除。
    pub(crate) fn key(&self) -> String {
        format!("{}/{:010}/{}-{}/{}", self.study_id, self.node_id, self.mine.iccs(), self.reference.iccs(), self.kind.slug())
    }
}

/// 一组对比的全部问题。
#[derive(Clone, Debug, Deserialize)]
pub struct AnswersRequest {
    pub study_id: String,
    pub node_id: NodeId,
    pub mine: Move,
    pub reference: Move,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AskQuestion {
    pub kind: QuestionKind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AskMove {
    pub notation: String,
    pub iccs: String,
    /// 从比较起点开始的引擎变化（中文记谱，第一着就是这一步）；没有搜索结果时为空。
    pub line: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AskAnalysis {
    /// 有两步棋的同一次搜索结果；为否时只有规则事实。
    pub searched: bool,
    /// 搜索完成了整轮。
    pub complete: bool,
    pub conditions: Option<String>,
    /// 按证据质量允许表达的范围。
    pub allowed_scope: Vec<String>,
}

/// 交给模型的证据包。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AskPack {
    pub pack_version: u32,
    pub compare_version: u32,
    pub question: AskQuestion,
    pub mover: String,
    pub fen_before: String,
    /// 比较起点之前的着法（中文记谱，最近若干着）。
    pub history: Vec<String>,
    pub history_complete: bool,
    pub mine: AskMove,
    pub reference: AskMove,
    /// 对照选择与棋谱、分析的关系。
    pub reference_relation: String,
    pub score_perspective: String,
    pub analysis: AskAnalysis,
    pub claims: Vec<AllowedClaim>,
    pub warnings: Vec<String>,
}

/// 回答下方的演示：从比较起点开始的一段变化，由程序按问题类型选择。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerDemo {
    pub label: String,
    pub moves: Vec<Move>,
    pub notation: Vec<String>,
}

/// 证据包及后台保留的对应关系（证据与演示不发给模型）。
#[derive(Clone, Debug)]
pub struct AskBundle {
    pub question: Question,
    pub pack: AskPack,
    /// 证据版本：证据包内容的指纹。
    pub revision: String,
    pub evidence: Vec<Evidence>,
    pub demos: Vec<AnswerDemo>,
}

impl AskBundle {
    pub fn claim(&self, id: &str) -> Option<&AllowedClaim> { self.pack.claims.iter().find(|c| c.id == id) }
}

/// 通过检查的回答。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoachAnswer {
    /// 直接回答问题。
    pub answer: CoachText,
    /// 补充的要点（最多三条）。
    pub points: Vec<CoachText>,
    /// 留给学习者自己在棋盘上验证的一个问题。
    pub check: String,
}

/// 一次成功的回答，连同引用的结论、证据与演示一起保存：对比依据之后更新，仍按回答时的依据显示。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerResult {
    pub output: CoachAnswer,
    pub claims: Vec<AllowedClaim>,
    pub evidence: Vec<Evidence>,
    pub demos: Vec<AnswerDemo>,
    pub evidence_revision: String,
    pub cache_key: String,
    pub model_label: String,
    pub audience: Audience,
    /// 回答依据的分析条件；只有规则事实时为空。
    pub conditions: Option<String>,
    pub created_at: i64,
}

/// 一个问题的状态与最近一次回答。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub question: Question,
    pub state: CoachState,
    pub generation: u64,
    pub result: Option<AnswerResult>,
    pub error: Option<String>,
    pub updated_at: i64,
}

/// 界面看到的回答状态（含义与 `CoachView` 相同）。
#[derive(Clone, Debug, Serialize)]
pub struct AnswerView {
    /// 状态版本：回答每次变化都会递增，界面按它判断新旧。
    pub seq: u64,
    pub question: Question,
    pub status: CoachStatus,
    pub reason: Option<String>,
    pub record: Option<AnswerRecord>,
    /// 记录中的回答是否对应当前的对比依据、模型与讲解设置。
    pub result_current: bool,
}

pub(crate) struct AskJob {
    question: Question,
    generation: u64,
    config_rev: u64,
    bundle: AskBundle,
    cache_key: String,
    settings: ProviderSettings,
    audience: Audience,
}

fn relation(kind: ReferenceKind) -> &'static str {
    match kind {
        ReferenceKind::Actual => "棋谱中的实际着法",
        ReferenceKind::Branch => "棋谱中的另一个分支",
        ReferenceKind::Candidate => "引擎候选",
        ReferenceKind::Other => "学习者另外指定的着法",
    }
}

fn allowed_scope(searched: bool, complete: bool, history_complete: bool) -> Vec<String> {
    let mut scope = vec![
        "只能使用 claims 中的结论；不得新增输入中没有的走法、变化、评分、胜率或将杀步数".to_string(),
        "评分只代表当前搜索条件下的评估，不能换算成胜率".to_string(),
    ];
    scope.push(
        match (searched, complete) {
            (false, _) => "没有两步棋的搜索结果：只说明规则事实，不比较优劣，也不推测对方的应对",
            (true, false) => "比较搜索没有完成整轮：不评价优劣，变化与回应只作参考",
            (true, true) => "两步的评分与优劣方向由程序在 difference、score 结论中给出，文字中不得改写或复述",
        }
        .to_string(),
    );
    if !history_complete {
        scope.push("此前的着法历史不完整：不得断言长将、长捉或重复局面的判罚".to_string());
    }
    scope
}

/// 一方的变化作为证据（可以演示）；没有搜索结果时为空。
fn line_evidence(id: &str, side: &CompareSide, certainty: Certainty) -> Option<Evidence> {
    let line = side.line.as_ref()?;
    Some(Evidence {
        id: id.to_string(),
        kind: EvidenceKind::Variation,
        topic: "line".into(),
        text: format!("从比较起点开始：{}", line.notation.join(" ")),
        squares: vec![side.mv.from, side.mv.to],
        arrows: vec![side.mv],
        line: Some(line.clone()),
        certainty,
        on: Some(LineBase::Parent),
    })
}

/// 一方的演示：有搜索变化时演示整条变化，否则只演示这一步。
fn demo_of(label: String, side: &CompareSide) -> AnswerDemo {
    match &side.line {
        Some(LineRef { moves, notation, .. }) => AnswerDemo { label, moves: moves.clone(), notation: notation.clone() },
        None => AnswerDemo { label, moves: vec![side.mv], notation: vec![side.notation.clone()] },
    }
}

/// 由已经得到的对比生成证据包。证据不足以回答这个问题时返回错误。
pub(crate) fn build(question: &Question, cached: &CachedComparison) -> Result<AskBundle, String> {
    let c = &cached.comparison;
    let study = &cached.study;
    let (mine, reference) = (&c.mine, &c.reference);
    let mut moves = Vec::new();
    for id in study.path(question.node_id).map_err(|e| e.to_string())?.into_iter().skip(1) {
        let n = study.node(id).map_err(|e| e.to_string())?;
        if n.gap {
            moves.push("（局面不连续）".to_string());
        } else if let Some(text) = &n.notation {
            moves.push(text.clone());
        }
    }
    let history = moves.split_off(moves.len().saturating_sub(HISTORY_LIMIT));
    let history_complete = study.history_complete_at(question.node_id).map_err(|e| e.to_string())?;
    let fen_before = study.node(question.node_id).map_err(|e| e.to_string())?.fen.clone();
    let search_certainty = if cached.complete { Certainty::Supported } else { Certainty::Tentative };

    let mut evidence = c.evidence.clone();
    evidence.extend(line_evidence("line-a", mine, search_certainty));
    evidence.extend(line_evidence("line-b", reference, search_certainty));
    let mut claims: Vec<AllowedClaim> = Vec::new();
    let mut add = |kind: &str, group: &str, statement: String, ids: Vec<String>, certainty: Certainty| {
        let id = format!("c{}", claims.len() + 1);
        claims.push(AllowedClaim { id, kind: kind.to_string(), group: group.to_string(), statement, evidence_ids: ids, certainty });
    };
    let topic = |ids: &[String]| {
        ids.first().and_then(|id| c.evidence.iter().find(|e| &e.id == id)).map(|e| e.topic.clone()).unwrap_or_else(|| "statement".into())
    };
    for (group, side) in [("mine", mine), ("reference", reference)] {
        for Claim { text, evidence: ids } in &side.changes {
            add(&topic(ids), group, format!("{}：{text}", side.notation), ids.clone(), weakest(&c.evidence, ids));
        }
    }
    for (group, side, line) in [("mine_reply", mine, "line-a"), ("reference_reply", reference, "line-b")] {
        if let Some(reply) = &side.reply {
            add("reply", group, format!("走{}之后，{}", side.notation, reply.text), vec![line.to_string()], search_certainty);
        }
    }
    for d in &c.differences {
        add("difference", "difference", d.text.clone(), d.evidence.clone(), search_certainty);
    }
    for (side, line) in [(mine, "line-a"), (reference, "line-b")] {
        if let Some(score) = &side.score_text {
            let ids = if side.line.is_some() { vec![line.to_string()] } else { Vec::new() };
            add("score", "score", format!("同一次搜索中{}的评分：{score}", side.notation), ids, search_certainty);
        }
    }
    let kind = question.kind;
    if !claims.iter().any(|cl| kind.required_groups().contains(&cl.group.as_str())) {
        return Err(kind.missing().to_string());
    }

    let demos = match kind {
        QuestionKind::MyMove => vec![demo_of(format!("演示我的选择 {}", mine.notation), mine)],
        QuestionKind::Difference => vec![
            demo_of(format!("演示我的选择 {}", mine.notation), mine),
            demo_of(format!("演示对照选择 {}", reference.notation), reference),
        ],
        QuestionKind::Reply => vec![demo_of("演示对方的回应".to_string(), mine)],
    };
    let ask_move = |side: &CompareSide| AskMove {
        notation: side.notation.clone(),
        iccs: side.mv.iccs(),
        line: side.line.as_ref().map(|l| l.notation.clone()).unwrap_or_default(),
    };
    let mover = c.mover.label();
    let pack = AskPack {
        pack_version: ASK_PACK_VERSION,
        compare_version: COMPARE_VERSION,
        question: AskQuestion { kind, text: kind.text(&mine.notation, &reference.notation) },
        mover: mover.to_string(),
        fen_before,
        history,
        history_complete,
        mine: ask_move(mine),
        reference: ask_move(reference),
        reference_relation: relation(cached.reference_kind).to_string(),
        score_perspective: format!("所有评分都以比较起点的走棋方（{mover}）为视角，正数对{mover}有利"),
        analysis: AskAnalysis {
            searched: cached.searched,
            complete: cached.complete,
            conditions: c.conditions.clone(),
            allowed_scope: allowed_scope(cached.searched, cached.complete, history_complete),
        },
        claims,
        warnings: c.warnings.clone(),
    };
    let revision = fingerprint(&pack);
    Ok(AskBundle { question: question.clone(), pack, revision, evidence, demos })
}

/// 缓存键：证据版本、提示词与输出结构版本、模型配置与学习水平都相同时，回答可以直接复用。
fn cache_key(bundle: &AskBundle, settings: &ProviderSettings, audience: Audience) -> String {
    let text = format!(
        "ask|{}|{}|{}|{}|{audience:?}",
        bundle.revision,
        prompt::ASK_PROMPT_VERSION,
        prompt::ASK_OUTPUT_VERSION,
        settings.revision()
    );
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// 请求模型并检查回答；没有通过检查时，带着检查结果让模型修正（与 AI 讲解相同的次数）。
fn run_job(inner: &Inner, job: &AskJob) -> Result<CoachAnswer, String> {
    let ids: Vec<String> = job.bundle.pack.claims.iter().map(|c| c.id.clone()).collect();
    let schema = prompt::ask_output_schema(&ids);
    let mut messages = vec![
        Message::system(prompt::ask_system_prompt()),
        Message::user(prompt::ask_user_message(&job.bundle.pack, job.audience)),
    ];
    let current = || inner.ask_current(job);
    let mut errors = Vec::new();
    for attempt in 0..OUTPUT_ATTEMPTS {
        let content = inner.coach_call(&job.settings, &current, &messages, &schema)?;
        match validate::validate_answer(&content, &job.bundle) {
            Ok(out) => return Ok(out),
            Err(found) => {
                debug!("追问的回答第 {} 次没有通过检查：{found:?}", attempt + 1);
                messages.push(Message::assistant(content));
                messages.push(Message::user(prompt::repair_message(&found)));
                errors = found;
            }
        }
    }
    Err(format!("回答没有通过检查：{}", errors.join("；")))
}

/// 整理成保存的结果：附上引用到的结论与证据。
fn result_of(job: &AskJob, output: CoachAnswer) -> AnswerResult {
    let cited: HashSet<&String> = output.answer.claim_ids.iter().chain(output.points.iter().flat_map(|p| p.claim_ids.iter())).collect();
    let claims: Vec<AllowedClaim> = job.bundle.pack.claims.iter().filter(|c| cited.contains(&c.id)).cloned().collect();
    let evidence_ids: HashSet<&String> = claims.iter().flat_map(|c| c.evidence_ids.iter()).collect();
    AnswerResult {
        output,
        evidence: job.bundle.evidence.iter().filter(|e| evidence_ids.contains(&e.id)).cloned().collect(),
        claims,
        demos: job.bundle.demos.clone(),
        evidence_revision: job.bundle.revision.clone(),
        cache_key: job.cache_key.clone(),
        model_label: job.settings.label(),
        audience: job.audience,
        conditions: job.bundle.pack.analysis.conditions.clone(),
        created_at: now_ms(),
    }
}

impl Inner {
    fn ask_bundle(&self, q: &Question) -> ApiResult<AskBundle> {
        let cached = self.cached_comparison(&q.study_id, q.node_id, q.mine, q.reference)?;
        build(q, &cached).map_err(ApiError::from)
    }

    /// 任务仍是这个问题最新的请求，且模型服务设置没有改变。
    fn ask_current(&self, job: &AskJob) -> bool {
        lock(&self.coach.asks).get(&job.question.key()).map(|(g, _)| *g) == Some(job.generation)
            && self.coach.config_rev.load(Ordering::SeqCst) == job.config_rev
    }

    /// 修改并保存一个问题的记录，推送新的状态。记录的读写都在 `ask_lock` 内进行。
    fn update_answer(&self, q: &Question, f: impl FnOnce(&mut AnswerRecord)) {
        let mut record = self.store.load_answer(&q.key()).ok().flatten().unwrap_or_else(|| AnswerRecord {
            question: q.clone(),
            state: CoachState::Idle,
            generation: 0,
            result: None,
            error: None,
            updated_at: now_ms(),
        });
        f(&mut record);
        record.updated_at = now_ms();
        if let Err(e) = self.store.save_answer(&record) {
            warn!("保存追问的回答失败：{e}");
            return;
        }
        self.coach.ask_seq.fetch_add(1, Ordering::SeqCst);
        events::emit(self.events.as_ref(), events::COACH_ANSWER_UPDATED, &self.answer_view(q));
    }

    /// 提出一个问题。已有对应当前依据与设置的回答、或同样的请求正在进行时，不再调用模型。
    pub(crate) fn ask_request(&self, q: Question) -> ApiResult<AnswerView> {
        let (config_rev, settings) = self.coach_snapshot();
        let settings = settings.map_err(ApiError::from)?;
        let bundle = self.ask_bundle(&q)?;
        let audience = self.config_snapshot().coach.audience;
        let key = cache_key(&bundle, &settings, audience);
        let _guard = lock(&self.coach.ask_lock);
        let existing = self.store.load_answer(&q.key())?;
        let answered = existing.as_ref().and_then(|r| r.result.as_ref()).is_some_and(|r| r.cache_key == key);
        let asking = lock(&self.coach.asks).get(&q.key()).is_some_and(|(_, k)| *k == key);
        if answered || asking {
            return Ok(self.answer_view(&q));
        }
        let generation = self.coach.generation.fetch_add(1, Ordering::SeqCst) + 1;
        lock(&self.coach.asks).insert(q.key(), (generation, key.clone()));
        self.update_answer(&q, |r| {
            r.state = CoachState::Queued;
            r.generation = generation;
            r.error = None;
        });
        let job = AskJob { question: q.clone(), generation, config_rev, bundle, cache_key: key, settings, audience };
        if self.coach_send(CoachTask::Ask(job)).is_err() {
            lock(&self.coach.asks).remove(&q.key());
            self.update_answer(&q, |r| {
                r.state = CoachState::Failed;
                r.error = Some("AI 讲解线程已停止".into());
            });
        }
        Ok(self.answer_view(&q))
    }

    /// 工作线程：发送前确认请求仍然有效，回答后写入记录。
    pub(crate) fn ask_run(&self, job: AskJob) {
        if !self.ask_current(&job) {
            let _guard = lock(&self.coach.ask_lock);
            // 仍是最新的请求，只是模型服务设置在发送前改变：不发送，说明原因
            if lock(&self.coach.asks).get(&job.question.key()).map(|(g, _)| *g) == Some(job.generation) {
                lock(&self.coach.asks).remove(&job.question.key());
                self.update_answer(&job.question, |r| {
                    r.state = CoachState::Failed;
                    r.error = Some("模型服务设置在发送前已改变，这个问题没有发送，可以重新提问".into());
                });
            }
            return;
        }
        self.coach_acquire();
        {
            let _guard = lock(&self.coach.ask_lock);
            if self.ask_current(&job) {
                self.update_answer(&job.question, |r| r.state = CoachState::Running);
            }
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(self, &job)))
            .unwrap_or_else(|_| Err("生成回答时出错".to_string()));
        self.coach_release();
        let _guard = lock(&self.coach.ask_lock);
        let key = job.question.key();
        {
            let mut asks = lock(&self.coach.asks);
            // 已被同一问题更新的请求取代，或所在的会话、节点已经删除
            if asks.get(&key).map(|(g, _)| *g) != Some(job.generation) {
                return;
            }
            asks.remove(&key);
        }
        match outcome {
            Ok(output) => {
                let result = result_of(&job, output);
                self.update_answer(&job.question, |r| {
                    r.state = CoachState::Ready;
                    r.result = Some(result);
                    r.error = None;
                });
            }
            Err(e) => self.update_answer(&job.question, |r| {
                r.state = CoachState::Failed;
                r.error = Some(e);
            }),
        }
    }

    /// 界面看到的状态：回答是否对应当前的对比依据、模型与讲解设置。
    pub(crate) fn answer_view(&self, q: &Question) -> AnswerView {
        // 先取版本再读取状态：读取期间若有变化，随后推送的事件版本更大
        let seq = self.coach.ask_seq.load(Ordering::SeqCst);
        let record = self.store.load_answer(&q.key()).ok().flatten();
        let settings = self.coach_settings();
        let audience = self.config_snapshot().coach.audience;
        let bundle = self.ask_bundle(q);
        let current = match (&settings, &bundle) {
            (Ok(s), Ok(b)) => Some((b.revision.clone(), cache_key(b, s, audience))),
            _ => None,
        };
        let is_current = |res: &AnswerResult| current.as_ref().is_some_and(|(_, key)| *key == res.cache_key);
        let (status, reason) = match &record {
            Some(r) if r.state == CoachState::Queued => (CoachStatus::Queued, None),
            Some(r) if r.state == CoachState::Running => (CoachStatus::Running, None),
            Some(r) if r.result.as_ref().is_some_and(is_current) => {
                (CoachStatus::Ready, r.error.as_ref().map(|e| format!("最近一次重新回答失败：{e}")))
            }
            Some(r) if r.state == CoachState::Failed => (CoachStatus::Failed, r.error.clone()),
            Some(AnswerRecord { result: Some(res), .. }) => {
                let reason = match (&settings, &bundle, &current) {
                    (Err(e), _, _) => format!("AI 讲解当前不可用：{e}"),
                    (_, Err(e), _) => format!("对比的依据有变化（{}），可以等对比完成后重新回答", e.message),
                    (_, _, Some((revision, _))) if *revision != res.evidence_revision => {
                        "对比的依据已更新（搜索结果或讲解规则有变化），可以重新回答".to_string()
                    }
                    _ => "模型或讲解设置已改变，可以重新回答".to_string(),
                };
                (CoachStatus::Stale, Some(reason))
            }
            _ => match (&settings, &bundle) {
                (Err(e), _) => (CoachStatus::Disabled, Some(e.clone())),
                (_, Err(e)) => (CoachStatus::None, Some(e.message.clone())),
                _ => (CoachStatus::None, record.as_ref().and_then(|r| r.error.clone())),
            },
        };
        let result_current = record.as_ref().and_then(|r| r.result.as_ref()).is_some_and(is_current);
        AnswerView { seq, question: q.clone(), status, reason, record, result_current }
    }

    /// 一组对比已有的问题与回答（没有提过的问题不列出）。
    pub(crate) fn answers_for(&self, req: &AnswersRequest) -> ApiResult<Vec<AnswerView>> {
        self.with_study(&req.study_id, |s| Ok(s.node(req.node_id).map(|_| ())?))?;
        let mut out = Vec::new();
        for kind in QuestionKind::ALL {
            let q = Question { study_id: req.study_id.clone(), node_id: req.node_id, mine: req.mine, reference: req.reference, kind };
            if self.store.load_answer(&q.key())?.is_some() {
                out.push(self.answer_view(&q));
            }
        }
        Ok(out)
    }

    /// 会话或节点删除前，丢弃对应的进行中的问题。记录的写入都在 `ask_lock` 内先确认请求仍然有效，
    /// 丢弃之后，返回的回答与“回答中”的状态都不再写入（不会在删除后留下记录）。
    pub(crate) fn ask_forget(&self, study_id: &str, nodes: Option<&[NodeId]>) {
        let prefixes: Vec<String> = match nodes {
            None => vec![format!("{study_id}/")],
            Some(ids) => ids.iter().map(|n| format!("{study_id}/{n:010}/")).collect(),
        };
        let _guard = lock(&self.coach.ask_lock);
        lock(&self.coach.asks).retain(|k, _| !prefixes.iter().any(|p| k.starts_with(p)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::test_support::*;

    #[test]
    fn evidence_comes_from_the_comparison_and_demos_follow_the_question() {
        let mine = ask_bundle(QuestionKind::MyMove, true, true);
        let p = &mine.pack;
        assert_eq!((p.mine.notation.as_str(), p.reference.notation.as_str()), ("炮二平五", "马二进三"));
        assert_eq!(p.question.text, "我的选择炮二平五起了什么作用？");
        assert_eq!(p.mine.line, vec!["炮二平五", "马8进7"]);
        assert_eq!(p.reference_relation, "引擎候选");
        for group in ["mine", "reference", "mine_reply", "reference_reply", "difference", "score"] {
            assert!(p.claims.iter().any(|c| c.group == group), "缺少 {group} 结论：{:?}", p.claims);
        }
        // 结论引用的证据都在后台保留的证据中；回应与评分关联到可以演示的变化
        for c in &p.claims {
            assert!(c.evidence_ids.iter().all(|id| mine.evidence.iter().any(|e| &e.id == id)), "{c:?}");
        }
        let reply = p.claims.iter().find(|c| c.group == "mine_reply").unwrap();
        assert_eq!(reply.statement, "走炮二平五之后，对方最值得关注的回应是马8进7");
        assert!(mine.evidence.iter().any(|e| e.id == "line-a" && e.line.is_some()));
        // 演示由程序按问题类型选择
        assert_eq!(mine.demos.len(), 1);
        assert_eq!(mine.demos[0].moves, vec![mv("h2e2"), mv("h9g7")]);
        let diff = ask_bundle(QuestionKind::Difference, true, true);
        assert_eq!(diff.demos.iter().map(|d| d.label.as_str()).collect::<Vec<_>>(), ["演示我的选择 炮二平五", "演示对照选择 马二进三"]);
        assert_eq!(ask_bundle(QuestionKind::Reply, true, true).demos[0].label, "演示对方的回应");
        // 证据版本：同样的内容不变；问题类型、搜索是否完整不同，版本不同
        assert_eq!(mine.revision, ask_bundle(QuestionKind::MyMove, true, true).revision);
        assert_ne!(mine.revision, diff.revision);
        assert_ne!(mine.revision, ask_bundle(QuestionKind::MyMove, true, false).revision);
        assert!(p.analysis.allowed_scope.iter().any(|s| s.contains("由程序在 difference、score 结论中给出")));
    }

    #[test]
    fn without_search_only_rule_facts_are_used_and_some_questions_cannot_be_answered() {
        let facts = ask_bundle(QuestionKind::MyMove, false, false);
        assert!(!facts.pack.analysis.searched);
        assert!(facts.pack.analysis.allowed_scope.iter().any(|s| s.contains("只说明规则事实")));
        assert!(facts.pack.claims.iter().all(|c| !matches!(c.group.as_str(), "mine_reply" | "reference_reply" | "score")));
        // 没有搜索变化时只演示这一步
        assert_eq!(facts.demos[0].moves, vec![mv("h2e2")]);
        // 没有能回放的对方回应：不能回答“对方的回应”
        let study = study_with(&[]);
        let ctx = xq_core::explain::ExplainContext { history_complete: true, repetition_count: 1 };
        let comparison =
            xq_core::explain::compare::compare_moves(&xq_core::Position::startpos(), mv("h2e2"), mv("h0g2"), &ctx, None).unwrap();
        let cached = CachedComparison { study, comparison, reference_kind: ReferenceKind::Candidate, searched: false, complete: false };
        let q = Question { study_id: "s".into(), node_id: 0, mine: mv("h2e2"), reference: mv("h0g2"), kind: QuestionKind::Reply };
        assert_eq!(build(&q, &cached).unwrap_err(), QuestionKind::Reply.missing());
        // 不完整的搜索：只作参考
        let partial = ask_bundle(QuestionKind::Difference, true, false);
        assert!(partial.pack.analysis.allowed_scope.iter().any(|s| s.contains("没有完成整轮")));
        assert!(partial.pack.claims.iter().filter(|c| c.group == "difference").all(|c| c.certainty == Certainty::Tentative));
    }

    #[test]
    fn question_keys_belong_to_the_comparison_origin() {
        let q = Question { study_id: "s1".into(), node_id: 12, mine: mv("h2e2"), reference: mv("h0g2"), kind: QuestionKind::Reply };
        assert_eq!(q.key(), "s1/0000000012/h2e2-h0g2/reply");
        // 与讲解记录的键前缀相同，删除节点时一起删除
        assert!(q.key().starts_with("s1/0000000012/"));
    }
}
