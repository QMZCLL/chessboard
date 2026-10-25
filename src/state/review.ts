// 复习：卡片列表、当前复习的卡片与这一次的选择。
// 复习时先只显示局面，用户在棋盘上重新选择（或直接看答案）后，才显示原来的思考与讲解。
import { computed, reactive } from "vue";
import { api, errorMessage } from "../api/backend";
import type * as T from "../api/types";
import { counts, filterItems, nextTodoId, type ReviewFilter } from "./reviewList";
import { nodeOf, openStudy, selectNode, setMode, study, wb } from "./workbench";

export const review = reactive({
  items: [] as T.ReviewItem[],
  loaded: false,
  filter: "todo" as ReviewFilter,
  current: null as T.ReviewDetail | null,
  /** choose：在棋盘上重新选择；revealed：已经选择或直接看了答案 */
  phase: "choose" as "choose" | "revealed",
  /** 这一次的选择（直接看答案时为空）与是否与棋谱相同 */
  attempt: null as { choice: T.ReviewMove | null; matched: boolean } | null,
  busy: false,
  error: null as string | null,
  note: { text: "", state: "idle" as "idle" | "editing" | "saving" | "saved" | "failed", error: null as string | null },
});

export const reviewCounts = computed(() => counts(review.items));
export const shownItems = computed(() => filterItems(review.items, review.filter));

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

function upsert(item: T.ReviewItem) {
  const i = review.items.findIndex((x) => x.card.id === item.card.id);
  if (i >= 0) review.items[i] = item;
  else review.items.push(item);
  if (review.current?.card.id === item.card.id) Object.assign(review.current, item);
  // 讲解面板显示这一着的复习状态
  if (wb.detail && wb.detail.study_id === item.card.study_id && wb.detail.node.id === item.card.node_id) wb.detail.review = item.card.status;
}

export async function loadReviews() {
  try {
    review.items = await api.listReviews();
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

/** 开始复习一张卡片：只显示局面，等待重新选择 */
export async function showCard(id: string) {
  review.busy = true;
  try {
    const detail = await api.reviewDetail(id);
    review.current = detail;
    review.phase = "choose";
    review.attempt = null;
    review.note = { text: detail.card.note, state: "idle", error: null };
    review.error = null;
    upsert(detail);
  } catch (e) {
    review.error = errorMessage(e);
  } finally {
    review.busy = false;
  }
}

/** 在棋盘上重新选择；choice 为空表示不走子、直接看答案 */
export async function submitChoice(choice: T.Move | null) {
  const current = review.current;
  if (!current || review.phase !== "choose" || review.busy) return;
  review.busy = true;
  try {
    const item = await api.reviewAttempt(current.card.id, choice);
    const last = item.card.attempts[item.card.attempts.length - 1];
    review.attempt = { choice: last?.choice ?? null, matched: !!last?.matched };
    review.phase = "revealed";
    upsert(item);
  } catch (e) {
    review.error = errorMessage(e);
  } finally {
    review.busy = false;
  }
}

/** 再试一次：回到只显示局面 */
export function retryCard() {
  review.phase = "choose";
  review.attempt = null;
}

export async function setStatus(status: T.ReviewStatus) {
  const current = review.current;
  if (!current) return;
  try {
    upsert(await api.updateReview(current.card.id, { status }));
  } catch (e) {
    review.error = errorMessage(e);
  }
}

export function editNote(text: string) {
  review.note.text = text;
  review.note.state = "editing";
}

export async function saveNote() {
  const current = review.current;
  if (!current || (review.note.state !== "editing" && review.note.state !== "failed")) return;
  const text = review.note.text;
  review.note.state = "saving";
  try {
    const item = await api.updateReview(current.card.id, { note: text });
    upsert(item);
    // 保存期间又有修改时保持“编辑中”
    if (review.note.text === text) review.note = { text, state: "saved", error: null };
    else review.note.state = "editing";
  } catch (e) {
    review.note.state = "failed";
    review.note.error = errorMessage(e);
  }
}

export async function deleteCard(id: string) {
  try {
    await api.deleteReview(id);
    const card = review.items.find((i) => i.card.id === id)?.card;
    review.items = review.items.filter((i) => i.card.id !== id);
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

export const nextCardId = computed(() => nextTodoId(shownItems.value, review.current?.card.id ?? null));

export async function showNext() {
  if (nextCardId.value) await showCard(nextCardId.value);
}

/** 打开原会话中的这一着（看谱学习，显示完整讲解） */
export async function openSource(item: T.ReviewItem) {
  await saveNote();
  wb.dialogs.review = false;
  if (wb.mode !== "auto") await setMode("auto");
  if (study.value?.id === item.card.study_id) await selectNode(item.card.node_id);
  else await openStudy(item.card.study_id, item.card.node_id);
}
