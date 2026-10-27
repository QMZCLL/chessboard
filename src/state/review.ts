// 复习：卡片列表、当前复习的卡片与这一次的选择。
// 复习时先只显示局面，用户在棋盘上重新选择（或直接看答案）后，才显示原来的思考与讲解。
// 练习目标分复现棋谱与局面决策；局面决策的比较在后台进行，完成后推送 review_updated 更新这次复习。
// 每张卡片有下一次复习的时间（标记状态时由后台安排）；“今天的练习”按顺序复习今天到期的卡片，结束后给出小结。
import { computed, reactive } from "vue";
import { api, errorMessage, on } from "../api/backend";
import type * as T from "../api/types";
import {
  counts,
  filterItems,
  isDue,
  newerItem,
  nextDueId,
  hasTopic,
  sessionSummary,
  topicLabel,
  type ReviewFilter,
  type SessionSummary,
  type TopicKey,
} from "./reviewList";
import { nodeOf, openStudy, safeStorage, selectNode, setMode, study, wb } from "./workbench";

const GOAL_KEY = "xq-review-goal";

type NoteDraft = { text: string; state: "idle" | "editing" | "saving" | "saved" | "failed"; error: string | null };

export const review = reactive({
  items: [] as T.ReviewItem[],
  loaded: false,
  filter: "today" as ReviewFilter,
  /** 计算“今天到期”所用的时刻：载入列表与每次修改卡片时更新 */
  clock: Date.now(),
  /** 一组练习：按顺序复习的卡片与开始的时间 */
  session: null as { ids: string[]; index: number; startedAt: number; label: string } | null,
  /** 刚结束的一组练习的小结 */
  summary: null as SessionSummary | null,
  current: null as T.ReviewDetail | null,
  /** choose：在棋盘上重新选择；revealed：已经选择或直接看了答案 */
  phase: "choose" as "choose" | "revealed",
  /** 练习目标（记住上次的选择） */
  goal: (safeStorage("get", GOAL_KEY) === "decision" ? "decision" : "recall") as T.PracticeGoal,
  /** 落子前选择的意图（可以不选） */
  intent: null as T.Intent | null,
  /** 这一次复习的时间：结果从当前卡片的复习记录中按它取出，局面决策的比较完成后随卡片更新 */
  attemptAt: null as number | null,
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

/** 打开复习；指定卡片时直接复习它（不在当前分组中时显示全部） */
export function openReviews(id?: string) {
  wb.dialogs.review = true;
  void loadReviews().then(() => {
    if (id && !shownItems.value.some((i) => i.card.id === id)) review.filter = "all";
    const target = id ?? (review.current ? null : (shownItems.value[0]?.card.id ?? null));
    if (target) void showCard(target);
  });
}

export const currentNote = computed<NoteDraft | null>(() => (review.current ? (review.notes[review.current.card.id] ?? null) : null));

/** 开始复习一张卡片：只显示局面，等待重新选择。快速切换时只采用最后一次打开的卡片。 */
export async function showCard(id: string) {
  const v = ++view;
  review.busy = true;
  try {
    const detail = await api.reviewDetail(id);
    // 视图代次只说明这是不是最后一次打开的卡片；详情还可能早于刚完成的修改读取，
    // 卡片与笔记都用合并之后较新的那份
    const latest = upsert(detail);
    if (v !== view) return;
    review.current = { ...detail, ...latest };
    review.phase = "choose";
    review.attemptAt = null;
    review.intent = null;
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

/** 在棋盘上重新选择；choice 为空表示不走子、直接看答案 */
export async function submitChoice(choice: T.Move | null) {
  const current = review.current;
  if (!current || review.phase !== "choose" || review.busy) return;
  const v = view;
  const id = current.card.id;
  review.busy = true;
  try {
    const { goal, intent } = review;
    const item = await queued(id, () => api.reviewAttempt(id, choice, goal, intent));
    upsert(item);
    // 等待期间换了卡片（或再试一次）：这次的结果只记在它自己的卡片上
    if (v !== view || review.current?.card.id !== id) return;
    review.attemptAt = item.card.attempts[item.card.attempts.length - 1]?.at ?? null;
    review.phase = "revealed";
  } catch (e) {
    if (v === view) review.error = errorMessage(e);
  } finally {
    if (v === view) review.busy = false;
  }
}

/** 再试一次：回到只显示局面 */
export function retryCard() {
  view += 1;
  review.phase = "choose";
  review.attemptAt = null;
  review.intent = null;
  review.busy = false;
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
    // 练习中删除的卡片不再排在这组里
    const s = review.session;
    if (s && s.ids.includes(id)) {
      const at = s.ids.indexOf(id);
      s.ids.splice(at, 1);
      if (at < s.index) s.index -= 1;
      if (!s.ids.length) review.session = null;
    }
    if (card && wb.detail && wb.detail.study_id === card.study_id && wb.detail.node.id === card.node_id) wb.detail.review = null;
    if (review.current?.card.id === id) {
      review.current = null;
      const next = shownItems.value[0]?.card.id;
      if (next) await showCard(next);
    }
  } catch (e) {
    review.error = errorMessage(e);
  }
}

/** 下一张：练习中按这组的顺序，否则是列表中下一张今天到期的卡片 */
export const nextCardId = computed(() => {
  const s = review.session;
  if (s) return s.ids[s.index + 1] ?? null;
  return nextDueId(shownItems.value, review.current?.card.id ?? null, review.clock);
});

export async function showNext() {
  const s = review.session;
  if (s) {
    if (s.index + 1 >= s.ids.length) return finishSession();
    s.index += 1;
    return showCard(s.ids[s.index]);
  }
  if (nextCardId.value) await showCard(nextCardId.value);
}

/** 开始一组练习：今天到期的卡片（按到期先后）、需要再练的卡片，或某个专题的卡片；size 为空时全部。 */
export function startSession(kind: "today" | "again" | TopicKey, size: number | null) {
  const now = Date.now();
  const pick = (i: T.ReviewItem) => (kind === "today" ? isDue(i.card, now) : kind === "again" ? i.card.status === "again" : hasTopic(i, kind));
  const pool = review.items.filter(pick);
  const ids = pool.slice(0, size ?? pool.length).map((i) => i.card.id);
  if (!ids.length) return;
  const label = kind === "today" ? "今天的练习" : kind === "again" ? "需要再练的卡片" : `专题：${topicLabel(kind)}`;
  review.session = { ids, index: 0, startedAt: now, label };
  review.summary = null;
  review.filter = kind === "today" || kind === "again" ? kind : "all";
  void showCard(ids[0]);
}

/** 结束这组练习，给出小结（只统计这组练习开始之后的复习）。 */
export function finishSession() {
  const s = review.session;
  if (!s) return;
  void saveNote();
  const cards = s.ids.map((id) => review.items.find((i) => i.card.id === id)?.card).filter((c): c is T.ReviewCard => !!c);
  review.summary = sessionSummary(cards, s.startedAt, Date.now());
  review.session = null;
}

export function closeSummary() {
  review.summary = null;
}

/** 打开原会话中的这一着（看谱学习，显示完整讲解） */
export async function openSource(item: T.ReviewItem) {
  await saveNote();
  wb.dialogs.review = false;
  if (wb.mode !== "auto") await setMode("auto");
  if (study.value?.id === item.card.study_id) await selectNode(item.card.node_id);
  else await openStudy(item.card.study_id, item.card.node_id);
}
