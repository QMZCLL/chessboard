// 复习卡片的分组、计数与文字说明。只依赖数据，便于单独测试。
// 列表与复习开始前都不显示棋谱中的着法、笔记等可能透露答案的内容。
// “今天到期”按本机日期计算：下一次复习的时间在今天结束之前（含已经过期的）。
import type { ArchiveSummary, CardKind, Confidence, DecisionFeedback, ReviewIssue, Theme, DecisionVerdict, Intent, PracticeGoal, PracticeSession, ReviewAttempt, ReviewCard, ReviewItem, ReviewStatus, ReviewThinking } from "../api/types";

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

/** 本机日期的下一个零点。夏令时切换的那天不是 24 小时，不能用零点加一天计算 */
function endOfDay(ts: number): number {
  const d = new Date(ts);
  d.setHours(24, 0, 0, 0);
  return d.getTime();
}

/** 今天到期：下一次复习的时间在本机今天结束之前 */
export const isDue = (card: Pick<ReviewCard, "due_at" | "created_at">, now: number) => dueAt(card) < endOfDay(now);

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

/** 题型 */
export const KIND_LABEL: Record<CardKind, string> = { move: "棋谱中的一着", threat: "找威胁", choice: "二选一", better: "找更好的着法" };

/** 生成的题目揭示后的结果说明；棋谱中的一着用 attemptText。 */
export function kindResultText(card: Pick<ReviewCard, "kind" | "targets" | "answer" | "played">, a: Pick<ReviewAttempt, "choice" | "marks" | "matched">): string {
  if (card.kind === "threat") {
    if (!a.marks?.length) return `这次没有点出格子，直接查看了答案：受到威胁的有 ${card.targets.length} 处。`;
    const found = a.marks.filter((m) => card.targets.includes(m)).length;
    const extra = a.marks.length - found;
    if (a.matched) return `点出了全部 ${card.targets.length} 处受到威胁的棋子。`;
    return `找到 ${found} / ${card.targets.length} 处${extra ? `，多点了 ${extra} 处` : ""}。`;
  }
  const played = card.played?.notation ?? "";
  if (!a.choice) return `这次没有选择，直接查看了答案：引擎首选是 ${card.answer.notation}，当时走了 ${played}。`;
  if (a.choice.notation === card.answer.notation) return `这次选了 ${a.choice.notation}，正是引擎首选（当时走了 ${played}）。`;
  if (a.choice.notation === played) return `这次选了 ${played}，与当时一样；引擎首选是 ${card.answer.notation}。`;
  return `这次走了 ${a.choice.notation}；引擎首选是 ${card.answer.notation}，当时走了 ${played}。`;
}

/** 作答时的把握：只用于以后找出“很有把握却没有选对”的地方，不影响这次的安排 */
export const CONFIDENCES = ["guess", "unsure", "sure"] as const satisfies readonly Confidence[];
export const CONFIDENCE_LABEL: Record<Confidence, string> = { guess: "基本靠猜", unsure: "有些把握", sure: "很有把握" };

/** 一次作答看了几级提示、选择的把握与用时（用时只记录，不评分）。 */
export function attemptNotesText(a: Pick<ReviewAttempt, "hints" | "confidence" | "elapsed_ms">): string {
  const parts: string[] = [];
  if (a.hints) parts.push(`看了 ${a.hints} 级提示`);
  if (a.confidence) parts.push(`选择时${CONFIDENCE_LABEL[a.confidence]}`);
  if (a.elapsed_ms !== null && a.elapsed_ms !== undefined) {
    const s = Math.round(a.elapsed_ms / 1000);
    parts.push(s < 60 ? `用时 ${s} 秒` : `用时 ${Math.floor(s / 60)} 分 ${s % 60} 秒`);
  }
  return parts.join(" · ");
}

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
  return a.decision?.support === "not_supported" ? "again" : null;
}

/** 最近几次复习中的一次：练习目标与结果的简短标记。 */
export function attemptMark(a: ReviewAttempt): { text: string; tone: "ok" | "miss" | "neutral" } {
  // 看了提示之后选对：记号相同，颜色是中性的
  if (a.goal === "recall" || !a.choice) return a.matched ? { text: "✓", tone: a.hints ? "neutral" : "ok" } : { text: "✗", tone: "miss" };
  // 局面决策按这次比较是否支持这个选择：与棋谱一致、但棋谱着法明显不如引擎参考时也是值得注意
  const s = a.decision?.support;
  if (s === "supported") return { text: "○", tone: a.hints ? "neutral" : "ok" };
  if (s === "not_supported") return { text: "△", tone: "miss" };
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

/**
 * 一次复习的结果（与后台安排间隔的规则相同，见 xq-app schedule.rs）：局面决策按这次比较是否支持这个选择，
 * 比较还没有结果（pending）或无法评价（ungraded）时不折算成选对或选错。
 */
export type AttemptOutcome = "independent" | "assisted" | "missed" | "revealed" | "pending" | "ungraded";

/** 与后台安排复习间隔的分法相同（见 schedule::outcome_of）：用了提示之后选对不算独立选对。 */
export function outcomeOf(a: ReviewAttempt): AttemptOutcome {
  // 没有走子、也没有点出格子（找威胁）：直接看了答案
  if (!a.choice && !a.marks?.length) return "revealed";
  const correct = a.hints > 0 ? "assisted" : "independent";
  if (a.goal === "recall") return a.matched ? correct : "missed";
  if (a.decision?.state === "pending") return "pending";
  const s = a.decision?.support;
  if (s === "supported") return correct;
  if (s === "not_supported") return "missed";
  return "ungraded";
}

export type Tone = "ok" | "miss" | "neutral";

/**
 * 一次作答的结果标签：练习中的列表、反馈与小结用同样的文字与颜色。复现棋谱按是否与棋谱一致；
 * 局面决策按这次比较是否支持这个选择，比较中与无法评价都是中性的，不当作选对或选错。
 */
export function outcomeBadge(a: ReviewAttempt, kind: CardKind = "move"): { text: string; tone: Tone } {
  const o = outcomeOf(a);
  if (o === "revealed") return { text: "直接看了答案", tone: "neutral" };
  if (o === "assisted") return { text: "看了提示后选对", tone: "neutral" };
  // 生成的题目：找威胁看是否全部找到，二选一看是否选了更好的一步（找更好的着法按局面决策的比较）
  if (kind === "threat") return o === "independent" ? { text: "全部找到", tone: "ok" } : { text: "没有全部找到", tone: "miss" };
  if (kind === "choice") return o === "independent" ? { text: "选对了", tone: "ok" } : { text: "没有选对", tone: "miss" };
  if (a.goal === "recall") return o === "independent" ? { text: "与棋谱一致", tone: "ok" } : { text: "与棋谱不同", tone: "miss" };
  const DECISION: Record<Exclude<AttemptOutcome, "revealed" | "assisted">, { text: string; tone: Tone }> = {
    independent: { text: "比较支持", tone: "ok" },
    missed: { text: "比较不支持", tone: "miss" },
    pending: { text: "比较中", tone: "neutral" },
    ungraded: { text: "无法评价", tone: "neutral" },
  };
  return DECISION[o];
}

/** 已经按最近一次复习标记了状态，但这次的比较还没有结果：下一次复习的时间在结果出来后确定 */
export function awaitingResult(card: ReviewCard): boolean {
  const last = card.attempts[card.attempts.length - 1];
  return !!last && last.decision?.state === "pending" && card.schedule_base?.attempt_at === last.at;
}

/** 一组练习中一道题的进展：todo 还没有作答；answered 已作答（含直接看答案）；skipped 跳过；missing 卡片已删除 */
export type EntryState = "todo" | "answered" | "skipped" | "missing";

/** 这张卡片在这组练习中的作答，按时间先后 */
export function practiceAttempts(card: Pick<ReviewCard, "attempts">, practiceId: string): ReviewAttempt[] {
  return card.attempts.filter((a) => a.practice === practiceId);
}

/** 一组练习中一道题的标签：作答过的按这组中的第一次作答 */
export function entryBadge(session: Pick<PracticeSession, "id" | "skipped">, cardId: string, card: ReviewCard | undefined): { text: string; tone: Tone } {
  const state = entryState(session, cardId, card);
  if (state === "answered" && card) return outcomeBadge(practiceAttempts(card, session.id)[0], card.kind);
  return { text: state === "missing" ? "卡片已删除" : state === "skipped" ? "已跳过" : "未作答", tone: "neutral" };
}

export function entryState(session: Pick<PracticeSession, "id" | "skipped">, cardId: string, card: ReviewCard | undefined): EntryState {
  if (!card) return "missing";
  if (practiceAttempts(card, session.id).length) return "answered";
  return session.skipped.includes(cardId) ? "skipped" : "todo";
}

/** 从 from（含）往后，第一道卡片还在的题目的位置；没有时为 null */
export function presentFrom(session: Pick<PracticeSession, "cards">, from: number, exists: (id: string) => boolean): number | null {
  for (let i = Math.max(0, from); i < session.cards.length; i++) if (exists(session.cards[i])) return i;
  return null;
}

export interface SessionSummary {
  /** 这组练习的张数（含练习中删除的卡片） */
  total: number;
  /** 这组练习中作答过的张数 */
  done: number;
  independent: number;
  /** 看了提示之后选对 */
  assisted: number;
  missed: number;
  revealed: number;
  /** 局面决策的比较还没有结果 */
  pending: number;
  /** 局面决策无法评价 */
  ungraded: number;
  /** 没有作答就跳过的张数 */
  skipped: number;
  /** 练习中删除的卡片 */
  missing: number;
  /** 作答过的卡片下一次复习的安排，按时间先后（比较还没有结果的另外说明） */
  schedule: { text: string; count: number }[];
}

/**
 * 一组练习的小结：只统计属于这组练习的作答，每张卡片按这组中的第一次作答计算
 * （揭示之后的“再试一次”是后续练习，不改写第一次的表现）；安排按卡片的最新状态，比较结果晚到时随之更新。
 */
export function practiceSummary(session: Pick<PracticeSession, "id" | "cards" | "skipped">, cards: ReviewCard[], now: number): SessionSummary {
  const summary: SessionSummary = { total: session.cards.length, done: 0, independent: 0, assisted: 0, missed: 0, revealed: 0, pending: 0, ungraded: 0, skipped: 0, missing: 0, schedule: [] };
  const byId = new Map(cards.map((c) => [c.id, c]));
  const groups = new Map<string, { due: number; count: number }>();
  for (const id of session.cards) {
    const card = byId.get(id);
    const state = entryState(session, id, card);
    if (state === "missing") summary.missing += 1;
    if (state === "skipped") summary.skipped += 1;
    if (state !== "answered" || !card) continue;
    summary.done += 1;
    summary[outcomeOf(practiceAttempts(card, session.id)[0])] += 1;
    const awaiting = awaitingResult(card);
    const text = awaiting ? "比较完成后确定" : dueText(card, now);
    // 还没有确定时间的排在最后
    const g = groups.get(text) ?? { due: awaiting ? Number.POSITIVE_INFINITY : dueAt(card), count: 0 };
    g.count += 1;
    groups.set(text, g);
  }
  summary.schedule = [...groups.entries()].sort((a, b) => a[1].due - b[1].due).map(([text, g]) => ({ text, count: g.count }));
  return summary;
}

/** 小结的文字。 */
export function summaryText(s: SessionSummary): string {
  const left = s.total - s.done - s.skipped - s.missing;
  const others = [s.skipped && `跳过 ${s.skipped} 张`, s.done && left > 0 && `还有 ${left} 张没有复习`, s.missing && `${s.missing} 张卡片已删除`].filter(Boolean);
  const rest = others.length ? `；${others.join("；")}` : "";
  if (!s.done) return `这组 ${s.total} 张还没有复习${rest}。`;
  const parts = [
    `${s.independent} 张独立选对`,
    `${s.assisted} 张看了提示后选对`,
    `${s.missed} 张没有选对`,
    `${s.revealed} 张直接看了答案`,
    `${s.pending} 张的比较还没有完成`,
    `${s.ungraded} 张无法评价`,
  ].filter((p) => !p.startsWith("0 "));
  const plan = s.schedule.map((g) => `${g.text} ${g.count} 张`).join("，");
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
