// 复习卡片的分组、计数与文字说明。只依赖数据，便于单独测试。
// 列表与复习开始前都不显示棋谱中的着法、笔记等可能透露答案的内容。
import type { ReviewCard, ReviewItem, ReviewStatus, ReviewThinking } from "../api/types";

export const STATUS_LABEL: Record<ReviewStatus, string> = { due: "待复习", again: "需要再练", understood: "已理解" };

/** todo：待复习与需要再练 */
export type ReviewFilter = "todo" | "understood" | "all";

export const isTodo = (status: ReviewStatus) => status !== "understood";

/** 卡片编号：与后台相同的“会话/节点” */
export const reviewCardId = (studyId: string, nodeId: number) => `${studyId}/${String(nodeId).padStart(10, "0")}`;

export function filterItems(items: ReviewItem[], filter: ReviewFilter): ReviewItem[] {
  if (filter === "all") return items;
  if (filter === "understood") return items.filter((i) => i.card.status === "understood");
  return items.filter((i) => isTodo(i.card.status));
}

export function counts(items: ReviewItem[]): Record<ReviewFilter, number> {
  const todo = items.filter((i) => isTodo(i.card.status)).length;
  return { todo, understood: items.length - todo, all: items.length };
}

/** 列表中的位置说明：会话、回合与走棋方（不含着法）。 */
export function positionLabel(card: Pick<ReviewCard, "turn" | "side">): string {
  return `第 ${card.turn} 回合 · ${card.side === "red" ? "红方" : "黑方"}走`;
}

/** 此前的着法（最后几着），让用户知道局面是怎么来的。 */
export function historyText(card: Pick<ReviewCard, "history" | "history_complete">, last = 6): string {
  const moves = card.history.map((m) => m.notation);
  if (!moves.length) return card.history_complete ? "从开局第一着开始" : "从这个局面开始（此前的历史未知）";
  const shown = moves.slice(-last).join(" ");
  return moves.length > last || !card.history_complete ? `此前：… ${shown}` : `此前：${shown}`;
}

/** 加入复习时的思考：第一次的选择与用了几级提示。 */
export function thinkingText(t: ReviewThinking | null): string {
  if (!t) return "加入复习时这一着没有先想后讲的记录。";
  const hints = t.hints_used ? `用了 ${t.hints_used} 级提示` : "没有用提示";
  if (t.revealed_directly || !t.first_choice) return `当时${hints}，没有走子就看了讲解。`;
  return `当时${hints}，第一步走了 ${t.first_choice.notation}，${t.matched ? "与棋谱相同" : "与棋谱不同"}。`;
}

/** 这次复习的结果。 */
export function attemptText(choice: { notation: string } | null, matched: boolean, answer: string): string {
  if (!choice) return `这次没有走子，直接查看了答案：棋谱中是 ${answer}。`;
  return matched ? `这次走了 ${choice.notation}，与棋谱相同。` : `这次走了 ${choice.notation}，棋谱中是 ${answer}。`;
}

/** 复习之后建议的状态：走对了建议标为已理解，否则建议再练。 */
export const suggestedStatus = (matched: boolean): ReviewStatus => (matched ? "understood" : "again");

/** 上次复习的时间。 */
export function whenText(ts: number | null, now = Date.now()): string {
  if (ts === null) return "还没有复习";
  const d = new Date(ts);
  const today = new Date(now);
  const sameDay = d.getFullYear() === today.getFullYear() && d.getMonth() === today.getMonth() && d.getDate() === today.getDate();
  const time = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return sameDay ? `今天 ${time} 复习过` : `${d.getMonth() + 1} 月 ${d.getDate()} 日复习过`;
}

/** 在当前筛选的列表中，排在这张卡片之后的下一张待复习卡片（到末尾时从头找）。 */
export function nextTodoId(items: ReviewItem[], currentId: string | null): string | null {
  const todo = items.filter((i) => isTodo(i.card.status) && i.card.id !== currentId);
  if (!todo.length) return null;
  const at = items.findIndex((i) => i.card.id === currentId);
  const after = todo.find((i) => items.indexOf(i) > at);
  return (after ?? todo[0]).card.id;
}

/**
 * 同一张卡片的两份数据中应采用的一份：修改次数多的较新，较早读取的数据不覆盖较新的。
 * 次数相同时卡片内容相同，采用后到的（原会话是否还在、当前名称可能已经改变）。
 */
export function newerItem(existing: ReviewItem | undefined, incoming: ReviewItem): ReviewItem {
  return existing && existing.card.revision > incoming.card.revision ? existing : incoming;
}
