// 复习：卡片列表、当前复习的卡片与这一次的选择。
// 复习时先只显示局面，用户在棋盘上重新选择（或直接看答案）后，才显示原来的思考与讲解。
// 练习目标分复现棋谱与局面决策；局面决策的比较在后台进行，完成后推送 review_updated 更新这次复习。
// 每张卡片有下一次复习的时间（标记状态时由后台安排）；“今天的练习”按顺序复习今天到期的卡片，结束后给出小结。
// 一组练习由后台保存（见 xq-app practice.rs）：卡片顺序在开始时固定，当前题目、跳过的题目与练习目标随练习写回，
// 退出或重新启动后从原处继续；这组练习中的作答带上批次编号，已经作答的题目恢复到揭示之后。
import { computed, reactive } from "vue";
import { api, errorMessage, newRequestId, on } from "../api/backend";
import type * as T from "../api/types";
import {
  counts,
  filterItems,
  isDue,
  newerItem,
  nextDueId,
  hasTopic,
  practiceAttempts,
  practiceSummary,
  presentFrom,
  topicLabel,
  type ReviewFilter,
  type SessionSummary,
  type TopicKey,
} from "./reviewList";
import { KINDS } from "./cardKind";
import { nodeOf, openStudy, safeStorage, selectNode, setMode, study, wb } from "./workbench";

const GOAL_KEY = "xq-review-goal";

type NoteDraft = { text: string; state: "idle" | "editing" | "saving" | "saved" | "failed"; error: string | null };

export const review = reactive({
  items: [] as T.ReviewItem[],
  loaded: false,
  filter: "today" as ReviewFilter,
  /** 计算“今天到期”所用的时刻：载入列表与每次修改卡片时更新 */
  clock: Date.now(),
  /** 进行中的一组练习（后台保存，重新启动后继续） */
  session: null as T.PracticeSession | null,
  /** 刚结束的一组练习：小结按卡片的最新状态计算，比较结果晚到时随之更新 */
  finished: null as T.PracticeSession | null,
  /** 练习中的说明（例如这组中有卡片已经删除、已跳过） */
  notice: null as string | null,
  /** 进入练习工作区之前的主区域：保存退出、完成后回到这里 */
  returnTo: "board" as "board" | "home",
  current: null as T.ReviewDetail | null,
  /** choose：在棋盘上重新选择；revealed：已经选择或直接看了答案 */
  phase: "choose" as "choose" | "revealed",
  /** 练习目标（记住上次的选择） */
  goal: (safeStorage("get", GOAL_KEY) === "decision" ? "decision" : "recall") as T.PracticeGoal,
  /** 落子前选择的意图（可以不选） */
  intent: null as T.Intent | null,
  /** 这次作答已经看了几级提示（0–3）；用了提示之后选对不算独立选对 */
  hints: 0,
  /** 这张卡片的分级提示：第一次看提示时读取 */
  hintData: null as T.ThinkHints | null,
  /** 作答时选择的信心（可以不选） */
  confidence: null as T.Confidence | null,
  /** 找威胁中点出的格子 */
  marks: [] as T.Square[],
  /** 这次作答开始的时刻：显示局面（或再试一次）时记下，作答时算出耗时，只记录不评分 */
  shownAt: Date.now(),
  /** 这一次复习的时间：结果从当前卡片的复习记录中按它取出，局面决策的比较完成后随卡片更新 */
  attemptAt: null as number | null,
  /** 这一次作答的请求编号：每次进入选择时重新生成；同一次作答重复提交（重试）时后台只记一次 */
  request: newRequestId(),
  busy: false,
  error: null as string | null,
  /** 理解或疑问的草稿与保存状态，按卡片保存：切换卡片后，迟到的保存结果只影响它自己的卡片 */
  notes: {} as Record<string, NoteDraft>,
});

/**
 * 当前视图的代次：每次打开一张卡片、再试一次都会递增。请求发出时记下代次，
 * 响应回来时代次已经改变，就只更新列表中的卡片，不改动正在复习的卡片的揭示状态、结果与提示。
 */
let view = 0;

/**
 * 同一张卡片的修改（笔记、状态、复习记录）依次发送：后一次修改等前一次完成后再发出，
 * 响应也按顺序到达，列表中的卡片不会被较早的响应覆盖。
 */
const writes = new Map<string, Promise<unknown>>();
function queued<R>(id: string, task: () => Promise<R>): Promise<R> {
  const run = (writes.get(id) ?? Promise.resolve()).catch(() => undefined).then(task);
  writes.set(id, run);
  // 清理不改变结果；失败由调用方处理，这里不再产生未处理的拒绝
  const cleanup = () => {
    if (writes.get(id) === run) writes.delete(id);
  };
  void run.then(cleanup, cleanup);
  return run;
}

export const reviewCounts = computed(() => counts(review.items, review.clock));

/** 这一次复习（揭示之后） */
export const currentAttempt = computed<T.ReviewAttempt | null>(
  () => review.current?.card.attempts.find((a) => a.at === review.attemptAt) ?? null,
);

export function setGoal(goal: T.PracticeGoal) {
  review.goal = goal;
  safeStorage("set", GOAL_KEY, goal);
  // 练习中改了目标：这组练习记住新的目标
  if (review.session && review.session.goal !== goal) {
    review.session.goal = goal;
    void savePractice({ goal });
  }
}

/** 这组练习中当前题目的卡片 */
export const sessionCardId = computed(() => (review.session ? (review.session.cards[review.session.index] ?? null) : null));

const cardExists = (id: string) => review.items.some((i) => i.card.id === id);

/**
 * 采用后台返回的一组练习：同一组的两份数据中修改次数少的不覆盖较新的；已经结束的不再作为进行中的练习。
 * 进行中的练习的目标就是复习时的目标。
 */
function adoptSession(s: T.PracticeSession | null) {
  const cur = review.session;
  if (s && cur && cur.id === s.id && cur.revision > s.revision) return;
  if (!s || s.finished_at !== null) {
    if (!s || cur?.id === s.id) review.session = null;
    return;
  }
  review.session = s;
  review.goal = s.goal;
}

/**
 * 还在发送的练习修改：换题、跳过、改目标都先改本地再写回，写回期间本地已经是较新的状态，
 * 较早的响应与读取不覆盖它；最后一次修改的响应包含全部修改。
 */
let practiceWrites = 0;

/** 把这组练习的修改写回后台；同一组的修改依次发送，失败时提示 */
async function savePractice(patch: { index?: number; skip?: string; goal?: T.PracticeGoal; finish?: boolean }): Promise<T.PracticeSession | null> {
  const s = review.session ?? review.finished;
  if (!s) return null;
  practiceWrites += 1;
  try {
    const saved = await queued(`practice:${s.id}`, () => api.updatePractice(s.id, patch));
    if (practiceWrites === 1) {
      if (review.session?.id === saved.id) adoptSession(saved);
      if (review.finished?.id === saved.id) review.finished = saved;
    }
    return saved;
  } catch (e) {
    review.error = errorMessage(e);
    return null;
  } finally {
    practiceWrites -= 1;
  }
}

/** 读取还没有结束的一组练习（启动与打开复习时）；本地的修改还在写回时不采用读到的 */
async function loadPractice() {
  try {
    const s = await api.currentPractice();
    if (!practiceWrites) adoptSession(s);
  } catch (e) {
    review.error = errorMessage(e);
  }
}

let subscribed = false;
function subscribe() {
  if (subscribed) return;
  subscribed = true;
  // 局面决策的比较完成后写回这次复习：按修改次数合并，较早的推送不覆盖较新的卡片
  on<T.ReviewItem>("review_updated", (item) => void upsert(item));
}
export const shownItems = computed(() => filterItems(review.items, review.filter, review.clock));

/** 这盘棋中收藏了、但还没有加入复习的着法 */
export const starredNotInReview = computed(() => {
  const s = study.value;
  if (!s) return [];
  const inReview = new Set(review.items.filter((i) => i.card.study_id === s.id).map((i) => i.card.node_id));
  return Object.values(s.nodes)
    .filter((n) => n.starred && n.mv && !inReview.has(n.id))
    .map((n) => n.id)
    .sort((a, b) => (nodeOf(a)?.ply ?? 0) - (nodeOf(b)?.ply ?? 0) || a - b);
});

/**
 * 采用后台返回的卡片，返回合并之后应显示的卡片：较早读取的卡片（修改次数更少）不覆盖列表与当前卡片中较新的。
 * 调用方接着显示这张卡片、初始化笔记时都用返回值，不直接用响应中的数据。
 */
function upsert(item: T.ReviewItem): T.ReviewItem {
  const i = review.items.findIndex((x) => x.card.id === item.card.id);
  const kept = newerItem(i >= 0 ? review.items[i] : undefined, item);
  if (kept !== item) return kept;
  if (i >= 0) review.items[i] = item;
  else review.items.push(item);
  if (review.current?.card.id === item.card.id) Object.assign(review.current, item);
  review.clock = Date.now();
  // 讲解面板显示这一着的复习状态
  if (wb.detail && wb.detail.study_id === item.card.study_id && wb.detail.node.id === item.card.node_id) wb.detail.review = item.card.status;
  return item;
}

export async function loadReviews() {
  subscribe();
  void loadPractice();
  try {
    const fresh = await api.listReviews();
    // 读取列表期间完成的修改不被较早读到的列表覆盖
    const known = new Map(review.items.map((i) => [i.card.id, i]));
    review.items = fresh.map((i) => newerItem(known.get(i.card.id), i));
    review.clock = Date.now();
    review.loaded = true;
    review.error = null;
  } catch (e) {
    review.error = errorMessage(e);
  }
}

/** 把一着加入复习，返回卡片 */
export async function addToReview(studyId: string, nodeId: number): Promise<T.ReviewItem> {
  const item = await api.addReview(studyId, nodeId);
  upsert(item);
  return item;
}

/** 把这盘棋收藏的着法都加入复习，返回新加入的张数 */
export async function addStarred(): Promise<number> {
  const s = study.value;
  if (!s) return 0;
  const ids = starredNotInReview.value;
  for (const id of ids) await addToReview(s.id, id);
  return ids.length;
}

/** 打开复习库；指定卡片时直接复习它（不在当前分组中时显示全部）。进行中的练习在工作区中继续 */
export function openReviews(id?: string) {
  wb.dialogs.review = true;
  void loadReviews().then(() => {
    if (id && !shownItems.value.some((i) => i.card.id === id)) review.filter = "all";
    const target = id ?? (review.current ? null : (shownItems.value[0]?.card.id ?? null));
    if (target) void showCard(target);
  });
}

export const currentNote = computed<NoteDraft | null>(() => (review.current ? (review.notes[review.current.card.id] ?? null) : null));

/**
 * 开始复习一张卡片：只显示局面，等待重新选择。快速切换时只采用最后一次打开的卡片。
 * restore 是一组练习的编号：这张卡片在这组练习中已经作答过时，恢复到最近一次作答揭示之后。
 */
export async function showCard(id: string, restore: string | null = null) {
  const v = ++view;
  review.busy = true;
  try {
    const detail = await api.reviewDetail(id);
    // 视图代次只说明这是不是最后一次打开的卡片；详情还可能早于刚完成的修改读取，
    // 卡片与笔记都用合并之后较新的那份
    const latest = upsert(detail);
    if (v !== view) return;
    review.current = { ...detail, ...latest };
    const mine = restore ? practiceAttempts(latest.card, restore) : [];
    const answered = mine[mine.length - 1];
    review.phase = answered ? "revealed" : "choose";
    review.attemptAt = answered?.at ?? null;
    resetAttempt();
    review.error = null;
    // 正在编辑或保存失败的草稿保留，其余按卡片中保存的内容显示
    const draft = review.notes[id];
    if (!draft || draft.state === "idle" || draft.state === "saved") review.notes[id] = { text: latest.card.note, state: "idle", error: null };
  } catch (e) {
    if (v === view) review.error = errorMessage(e);
  } finally {
    if (v === view) review.busy = false;
  }
}

/** 在棋盘上走子或在选项中选一个作答（棋谱中的一着、二选一、找更好的着法） */
export function submitChoice(choice: T.Move) {
  return submitAttempt(choice, []);
}

/** 找威胁：提交点出的格子 */
export function submitMarks() {
  if (!review.current || KINDS[review.current.card.kind].answer !== "pick" || !review.marks.length) return Promise.resolve();
  return submitAttempt(null, [...review.marks]);
}

/** 不作答、直接看答案：同样记为一次作答（与跳过不同） */
export function revealAnswer() {
  return submitAttempt(null, []);
}

/** 记下这次作答：走子或选择的着法、找威胁中点出的格子（两者都为空是直接看答案），以及提示、用时与信心 */
async function submitAttempt(choice: T.Move | null, marks: T.Square[]) {
  const current = review.current;
  if (!current || review.phase !== "choose" || review.busy) return;
  const v = view;
  const id = current.card.id;
  // 这组练习中的当前题目：作答记入这组练习
  const practice = review.session && sessionCardId.value === id ? review.session.id : null;
  const request = review.request;
  review.busy = true;
  try {
    const { goal, intent } = review;
    // 耗时只记录、不评分；离开很久的作答记为一小时
    const extra = { hints: review.hints, elapsed_ms: Math.min(Date.now() - review.shownAt, 3_600_000), confidence: review.confidence, marks };
    const send = () => queued(id, () => api.reviewAttempt(id, choice, goal, intent, practice, request, extra));
    // 这组练习的换题还在写回时，等写回之后再提交：后台只把当前题目的作答记入这组练习
    const item = await (practice ? queued(`practice:${practice}`, send) : send());
    upsert(item);
    // 等待期间换了卡片（或再试一次）：这次的结果只记在它自己的卡片上
    if (v !== view || review.current?.card.id !== id) return;
    const attempts = item.card.attempts;
    review.attemptAt = (attempts.find((a) => a.request === request) ?? attempts[attempts.length - 1])?.at ?? null;
    review.phase = "revealed";
  } catch (e) {
    if (v === view) review.error = errorMessage(e);
  } finally {
    if (v === view) review.busy = false;
  }
}

/** 开始一次新的作答：新的请求编号，意图、提示、信心、点出的格子与计时从头开始 */
function resetAttempt() {
  review.request = newRequestId();
  review.intent = null;
  review.hints = 0;
  review.hintData = null;
  review.confidence = null;
  review.marks = [];
  review.shownAt = Date.now();
}

/** 找威胁：点出或取消一个格子 */
export function toggleMark(sq: T.Square) {
  if (!review.current || KINDS[review.current.card.kind].answer !== "pick" || review.phase !== "choose" || review.busy) return;
  review.marks = review.marks.includes(sq) ? review.marks.filter((m) => m !== sq) : [...review.marks, sq];
}

/** 再试一次：回到只显示局面 */
export function retryCard() {
  view += 1;
  review.phase = "choose";
  review.attemptAt = null;
  resetAttempt();
  review.busy = false;
}

/** 再看一级提示（最多三级）：第一次时读取这张卡片的提示，读取期间换了卡片时不采用 */
export async function showReviewHint() {
  const current = review.current;
  if (!current || review.phase !== "choose" || review.hints >= 3) return;
  if (!review.hintData) {
    const v = view;
    try {
      const hints = await api.reviewHints(current.card.id);
      if (v !== view) return;
      review.hintData = hints;
    } catch (e) {
      if (v === view) review.error = errorMessage(e);
      return;
    }
  }
  if (review.hintData) review.hints = Math.min(3, review.hints + 1);
}

/**
 * 标记一张卡片的状态。卡片由调用方在操作开始时确定：先保存笔记再标记时，等待期间换了卡片，
 * 仍然标记原来的那张。失败时抛出，由调用方提示。
 */
export async function setStatus(id: string, status: T.ReviewStatus) {
  upsert(await queued(id, () => api.updateReview(id, { status })));
}

/** 保存用户确认的问题（整组替换），与这张卡片的其他修改依次发送。 */
export async function setIssues(id: string, issues: T.ReviewIssue[]) {
  upsert(await queued(id, () => api.updateReview(id, { issues })));
}

export function editNote(text: string) {
  const id = review.current?.card.id;
  if (!id) return;
  review.notes[id] = { text, state: "editing", error: null };
}

/** 保存一张卡片（默认当前卡片）的理解或疑问；排在这张卡片之前的修改之后发送，发送时取最新的草稿。 */
export function saveNote(id = review.current?.card.id): Promise<void> {
  if (!id) return Promise.resolve();
  return queued(id, async () => {
    const draft = review.notes[id];
    if (!draft || (draft.state !== "editing" && draft.state !== "failed")) return;
    const text = draft.text;
    draft.state = "saving";
    try {
      upsert(await api.updateReview(id, { note: text }));
      // 保存期间又有修改时保持“编辑中”
      const latest = review.notes[id];
      if (latest.text === text) review.notes[id] = { text, state: "saved", error: null };
      else latest.state = "editing";
    } catch (e) {
      review.notes[id] = { ...review.notes[id], state: "failed", error: errorMessage(e) };
    }
  });
}

export async function deleteCard(id: string) {
  try {
    await api.deleteReview(id);
    const card = review.items.find((i) => i.card.id === id)?.card;
    review.items = review.items.filter((i) => i.card.id !== id);
    if (card && wb.detail && wb.detail.study_id === card.study_id && wb.detail.node.id === card.node_id) wb.detail.review = null;
    if (review.current?.card.id === id) {
      review.current = null;
      // 练习中删除了当前这一题：这组练习的顺序不变，说明已删除，继续后面的题目
      if (review.session && sessionCardId.value === id) return await goTo(review.session.index);
      const next = shownItems.value[0]?.card.id;
      if (next) await showCard(next);
    }
  } catch (e) {
    review.error = errorMessage(e);
  }
}

/** 下一张：练习中按这组的顺序（跳过已删除的卡片），否则是列表中下一张今天到期的卡片 */
export const nextCardId = computed(() => {
  const s = review.session;
  if (s) {
    const at = presentFrom(s, s.index + 1, cardExists);
    return at === null ? null : s.cards[at];
  }
  return nextDueId(shownItems.value, review.current?.card.id ?? null, review.clock);
});

/** 显示这组练习的当前题目：卡片已删除时往后找；已经作答过的恢复到揭示之后 */
export async function resumeSession() {
  const s = review.session;
  if (s) await goTo(s.index);
}

/** 换到这组练习中的第 index 题（卡片已删除时往后找），并写回当前位置；后面没有题目时结束这组练习 */
export async function goTo(index: number, patch: { skip?: string } = {}) {
  const s = review.session;
  if (!s) return;
  const at = presentFrom(s, index, cardExists);
  // 中间有已经删除的卡片：说明已跳过，继续后面的题目
  const gone = s.cards.slice(Math.max(0, index), at ?? s.cards.length).map((_, i) => Math.max(0, index) + i + 1);
  review.notice = gone.length ? `第 ${gone.join("、")} 题的卡片已经删除，已跳过` : null;
  if (at === null) {
    if (patch.skip) await savePractice(patch);
    return finishSession();
  }
  const moved = at !== s.index;
  s.index = at;
  if (moved || patch.skip) void savePractice({ ...patch, ...(moved ? { index: at } : {}) });
  await showCard(s.cards[at], s.id);
}

/** 跳过这一题：没有作答就离开（与直接看答案不同，后者是一次作答），记入这组练习后进入下一题 */
export async function skipCard() {
  const s = review.session;
  const id = sessionCardId.value;
  if (!s || !id) return;
  if (!s.skipped.includes(id)) s.skipped.push(id);
  await goTo(s.index + 1, { skip: id });
}

export async function showNext() {
  const s = review.session;
  if (s) return goTo(s.index + 1);
  if (nextCardId.value) await showCard(nextCardId.value);
}

/**
 * 开始一组练习：今天到期的卡片（按到期先后）、需要再练的卡片，或某个专题的卡片；size 为空时全部。
 * 卡片的顺序在开始时确定，由后台保存；之前还没有结束的一组记为结束。返回是否开始了。
 */
async function startSession(kind: "today" | "again" | TopicKey, size: number | null): Promise<boolean> {
  const now = Date.now();
  const pick = (i: T.ReviewItem) => (kind === "today" ? isDue(i.card, now) : kind === "again" ? i.card.status === "again" : hasTopic(i, kind));
  const pool = review.items.filter(pick);
  const cards = pool.slice(0, size ?? pool.length).map((i) => i.card.id);
  if (!cards.length) return false;
  const label = kind === "today" ? "今天的练习" : kind === "again" ? "需要再练的卡片" : `专题：${topicLabel(kind)}`;
  const source: T.PracticeSource = kind === "today" || kind === "again" ? kind : "topic";
  try {
    const s = await api.startPractice({ label, source, topic: source === "topic" ? kind : null, goal: review.goal, cards });
    review.session = s;
    review.finished = null;
    review.filter = source === "topic" ? "all" : source;
    await showCard(s.cards[0], s.id);
    return true;
  } catch (e) {
    review.error = errorMessage(e);
    return false;
  }
}

function enterWorkspace() {
  if (wb.main !== "practice") review.returnTo = wb.main;
  wb.dialogs.review = false;
  wb.main = "practice";
}

/** 进入专注练习工作区，从这组练习的当前题目继续 */
export async function openPractice() {
  if (!review.session) return;
  enterWorkspace();
  await resumeSession();
}

/** 开始一组练习，并进入专注练习工作区；没有开始时（例如卡片刚被删除）留在原处，返回 false，原因见 review.error */
export async function startPractice(kind: "today" | "again" | TopicKey, size: number | null): Promise<boolean> {
  const started = await startSession(kind, size);
  if (started) enterWorkspace();
  return started;
}

/** 保存退出：这组练习的位置已经写回，之后可以从原处继续；回到进入练习之前的位置 */
export async function exitPractice() {
  await saveNote();
  review.finished = null;
  review.notice = null;
  wb.main = review.returnTo;
}

/** 结束这组练习，给出小结（只统计属于这组练习的作答）。 */
export async function finishSession() {
  const s = review.session;
  if (!s) return;
  void saveNote();
  review.finished = s;
  review.session = null;
  await savePractice({ finish: true });
}

/** 刚结束的一组练习的小结（只统计属于这组练习的作答） */
export const sessionResult = computed<SessionSummary | null>(() => {
  const f = review.finished;
  if (!f) return null;
  return practiceSummary(
    f,
    review.items.map((i) => i.card),
    review.clock,
  );
});

/** 打开原会话中的这一着（看谱学习，显示完整讲解） */
export async function openSource(item: T.ReviewItem) {
  await saveNote();
  wb.dialogs.review = false;
  wb.main = "board";
  if (wb.mode !== "auto") await setMode("auto");
  if (study.value?.id === item.card.study_id) await selectNode(item.card.node_id);
  else await openStudy(item.card.study_id, item.card.node_id);
}
