// 复习卡片的分组、计数与文字说明。只依赖数据，便于单独测试。
// 列表与复习开始前都不显示棋谱中的着法、笔记等可能透露答案的内容。
// “今天到期”按本机日期计算：下一次复习的时间在今天结束之前（含已经过期的）。
import type { ArchiveSummary, DecisionFeedback, ReviewIssue, Theme, DecisionVerdict, Intent, PracticeGoal, ReviewAttempt, ReviewCard, ReviewItem, ReviewStatus, ReviewThinking } from "../api/types";

export const STATUS_LABEL: Record<ReviewStatus, string> = { due: "待复习", again: "需要再练", understood: "已理解" };

/** today：今天到期（含已经过期的）；again：需要再练；understood：已理解；all：全部 */
export type ReviewFilter = "today" | "again" | "understood" | "all";

export const DAY_MS = 86_400_000;

function startOfDay(ts: number): number {
  const d = new Date(ts);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** 卡片下一次复习的时间（后台总会给出；没有时按加入时到期） */
export const dueAt = (card: Pick<ReviewCard, "due_at" | "created_at">) => card.due_at ?? card.created_at;

/** 今天到期：下一次复习的时间在本机今天结束之前 */
export const isDue = (card: Pick<ReviewCard, "due_at" | "created_at">, now: number) => dueAt(card) < startOfDay(now) + DAY_MS;

/** 下一次复习的说明（按本机日期）。 */
export function dueText(card: Pick<ReviewCard, "due_at" | "created_at">, now: number): string {
  const due = dueAt(card);
  const days = Math.round((startOfDay(due) - startOfDay(now)) / DAY_MS);
  if (days < 0) return `已到期 ${-days} 天`;
  if (days === 0) return "今天到期";
  if (days === 1) return "明天再看";
  if (days < 7) return `${days} 天后再看`;
  const d = new Date(due);
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日再看`;
}

/** 卡片编号：与后台相同的“会话/节点” */
export const reviewCardId = (studyId: string, nodeId: number) => `${studyId}/${String(nodeId).padStart(10, "0")}`;

export function filterItems(items: ReviewItem[], filter: ReviewFilter, now: number): ReviewItem[] {
  if (filter === "all") return items;
  if (filter === "today") return items.filter((i) => isDue(i.card, now));
  return items.filter((i) => i.card.status === filter);
}

export function counts(items: ReviewItem[], now: number): Record<ReviewFilter, number> {
  const n = (f: ReviewFilter) => filterItems(items, f, now).length;
  return { today: n("today"), again: n("again"), understood: n("understood"), all: items.length };
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

export const GOAL_LABEL: Record<PracticeGoal, string> = { recall: "复现棋谱", decision: "局面决策" };

export const INTENT_LABEL: Record<Intent, string> = {
  develop: "出子",
  defend: "防守",
  capture: "吃子",
  check: "将军",
  improve: "改善位置",
  other: "其他",
};
export const INTENTS = Object.keys(INTENT_LABEL) as Intent[];

export const VERDICT_TEXT: Record<DecisionVerdict, string> = {
  same_as_record: "与棋谱一致",
  reasonable: "与棋谱不同，但本次分析中与参考评分接近",
  noteworthy: "这个选择留下了一个值得注意的问题",
  unclear: "分析尚不充分，暂不评价走法优劣",
};

/** 局面决策反馈的标题（比较还在进行或没有得到结果时说明原因）。 */
export function decisionTitle(d: DecisionFeedback): string {
  if (d.state === "pending") return "正在用同一次搜索比较你的选择、棋谱着法与引擎参考…";
  if (d.state === "failed" || !d.verdict) return `没有得到比较结果：${d.error ?? "原因不明"}`;
  return VERDICT_TEXT[d.verdict];
}

/**
 * 复习之后建议的状态。复现棋谱按是否与棋谱一致；局面决策只在留下值得注意的问题时建议再练：
 * 与棋谱一致或评分接近不等于已经理解，由用户对照意图与实际变化后自己判断。
 */
export function suggestionFor(a: ReviewAttempt): ReviewStatus | null {
  if (a.goal === "recall") return suggestedStatus(a.matched);
  return a.decision?.verdict === "noteworthy" ? "again" : null;
}

/** 最近几次复习中的一次：练习目标与结果的简短标记。 */
export function attemptMark(a: ReviewAttempt): { text: string; tone: "ok" | "miss" | "neutral" } {
  if (a.goal === "recall" || !a.choice) return a.matched ? { text: "✓", tone: "ok" } : { text: "✗", tone: "miss" };
  const v = a.decision?.verdict;
  if (v === "same_as_record" || v === "reasonable") return { text: "○", tone: "ok" };
  if (v === "noteworthy") return { text: "△", tone: "miss" };
  return { text: "·", tone: "neutral" };
}

/** 上次复习的时间。 */
export function whenText(ts: number | null, now = Date.now()): string {
  if (ts === null) return "还没有复习";
  const d = new Date(ts);
  const today = new Date(now);
  const sameDay = d.getFullYear() === today.getFullYear() && d.getMonth() === today.getMonth() && d.getDate() === today.getDate();
  const time = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return sameDay ? `今天 ${time} 复习过` : `${d.getMonth() + 1} 月 ${d.getDate()} 日复习过`;
}

/** 在当前筛选的列表中，排在这张卡片之后的下一张今天到期的卡片（到末尾时从头找）。 */
export function nextDueId(items: ReviewItem[], currentId: string | null, now: number): string | null {
  const todo = items.filter((i) => isDue(i.card, now) && i.card.id !== currentId);
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

/** 一次复习的结果（与后台安排间隔的规则相同，见 xq-app schedule.rs） */
export type AttemptOutcome = "independent" | "missed" | "revealed";

export function outcomeOf(a: ReviewAttempt): AttemptOutcome {
  if (!a.choice) return "revealed";
  if (a.goal === "recall") return a.matched ? "independent" : "missed";
  const v = a.decision?.verdict;
  if (v === "same_as_record" || v === "reasonable") return "independent";
  if (v === "noteworthy") return "missed";
  return a.matched ? "independent" : "missed";
}

export interface SessionSummary {
  /** 这组练习的张数 */
  total: number;
  /** 这组练习中复习过的张数 */
  done: number;
  independent: number;
  missed: number;
  revealed: number;
  /** 复习过的卡片下一次复习的安排，按时间先后 */
  schedule: { text: string; count: number }[];
}

/** 一组练习的小结：数字都来自这组练习开始之后的复习记录与卡片的安排。 */
export function sessionSummary(cards: ReviewCard[], startedAt: number, now: number): SessionSummary {
  const summary: SessionSummary = { total: cards.length, done: 0, independent: 0, missed: 0, revealed: 0, schedule: [] };
  const groups = new Map<string, { due: number; count: number }>();
  for (const card of cards) {
    const inSession = card.attempts.filter((a) => a.at >= startedAt);
    const last = inSession[inSession.length - 1];
    if (!last) continue;
    summary.done += 1;
    summary[outcomeOf(last)] += 1;
    const text = dueText(card, now);
    const g = groups.get(text) ?? { due: dueAt(card), count: 0 };
    g.count += 1;
    groups.set(text, g);
  }
  summary.schedule = [...groups.entries()].sort((a, b) => a[1].due - b[1].due).map(([text, g]) => ({ text, count: g.count }));
  return summary;
}

/** 小结的文字。 */
export function summaryText(s: SessionSummary): string {
  if (!s.done) return `这组 ${s.total} 张还没有复习。`;
  const parts = [`${s.independent} 张独立选对`, `${s.missed} 张没有选对`, `${s.revealed} 张直接看了答案`].filter((p) => !p.startsWith("0 "));
  const plan = s.schedule.map((g) => `${g.text} ${g.count} 张`).join("，");
  const rest = s.total > s.done ? `；还有 ${s.total - s.done} 张没有复习` : "";
  return `本次完成 ${s.done} 张：${parts.join("，")}${rest}。安排：${plan}。`;
}

/** 设置中的复习间隔：“1, 3, 7”这样的文字转成天数（后台再检查范围、排序与去重）。 */
export function parseIntervals(text: string): number[] {
  return text
    .split(/[\s,，、]+/)
    .filter(Boolean)
    .map(Number)
    .filter((n) => Number.isInteger(n) && n > 0);
}

/** 导入学习档案前的预览说明。 */
export function archivePreviewLines(s: ArchiveSummary): string[] {
  const d = new Date(s.exported_at);
  const pad = (n: number) => String(n).padStart(2, "0");
  const when = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  const lines = [
    `导出于 ${when}（版本 ${s.app_version}）`,
    `学习会话：新增 ${s.studies_new} 个${s.studies_existing ? `；本机已有 ${s.studies_existing} 个，保留本机的版本` : ""}`,
    `复习卡片：新增 ${s.cards_new} 张；与本机相同 ${s.cards_same} 张；需要合并 ${s.cards_merge} 张`,
  ];
  if (s.cards_merge) lines.push("合并时历次复习按时间合在一起，状态、理解与安排取较新的一份");
  if (s.cards_orphan) lines.push(`其中 ${s.cards_orphan} 张卡片的原会话不在本机也不在档案中：照样导入，可以复习，只是不能打开原局面`);
  return lines;
}

export const THEME_LABEL: Record<Theme, string> = {
  king_safety: "应将与将帅安全",
  threatened: "受攻击棋子的处理",
  protection: "保护关系变化",
  lines: "炮架与线路变化",
  recapture: "吃子后的回吃关系",
};

export const ISSUE_LABEL: Record<ReviewIssue, string> = {
  missed_threat: "没有检查对方的直接威胁",
  missed_protection: "没有注意保护关系",
  missed_recapture: "没有算清吃子后的回吃",
  missed_lines: "没有注意线路与炮架",
  missed_reply: "没有考虑对方的回应",
};
export const ISSUES = Object.keys(ISSUE_LABEL) as ReviewIssue[];

/** 专题：程序建议的主题，或用户自己确认的问题 */
export type TopicKey = `theme:${Theme}` | `issue:${ReviewIssue}`;

export function topicLabel(key: TopicKey): string {
  const [kind, name] = key.split(":") as ["theme" | "issue", string];
  return kind === "theme" ? THEME_LABEL[name as Theme] : `我标记的：${ISSUE_LABEL[name as ReviewIssue]}`;
}

export function hasTopic(item: ReviewItem, key: TopicKey): boolean {
  const [kind, name] = key.split(":");
  return kind === "theme" ? item.themes.some((t) => t.theme === name) : item.card.issues.includes(name as ReviewIssue);
}

/** 有卡片的专题与张数：用户确认的问题在前，各自按张数从多到少。 */
export function topics(items: ReviewItem[]): { key: TopicKey; label: string; count: number }[] {
  const keys: TopicKey[] = [...ISSUES.map((i) => `issue:${i}` as const), ...(Object.keys(THEME_LABEL) as Theme[]).map((t) => `theme:${t}` as const)];
  const list = keys.map((key) => ({ key, label: topicLabel(key), count: items.filter((i) => hasTopic(i, key)).length })).filter((t) => t.count > 0);
  const rank = (k: TopicKey) => (k.startsWith("issue:") ? 0 : 1);
  return list.sort((a, b) => rank(a.key) - rank(b.key) || b.count - a.count);
}

/** 推荐练习：同一个问题标记在两张以上的卡片上（只按用户自己确认的问题推荐） */
export function recommendation(items: ReviewItem[]): { key: TopicKey; issue: ReviewIssue; count: number } | null {
  let best: { key: TopicKey; issue: ReviewIssue; count: number } | null = null;
  for (const issue of ISSUES) {
    const count = items.filter((i) => i.card.issues.includes(issue)).length;
    if (count >= 2 && (!best || count > best.count)) best = { key: `issue:${issue}`, issue, count };
  }
  return best;
}
