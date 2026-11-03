// 题型：每种卡片怎样作答、作答前怎样说明、揭示后怎样写出结果与标签。复习卡片、专注练习工作区与作答提交
// 都从这里取，不在各处按题型分支；以后加题型时在 KINDS 中加一项。只依赖数据，便于单独测试。
// 判断方式与后台一致（见 xq-app review.rs 的 review_attempt）：找威胁与二选一按是否答对（记为复现棋谱），
// 找更好的着法总是局面决策，只有棋谱中的一着按用户选择的练习目标。
import type { CardKind, DecisionFeedback, DecisionVerdict, Move, PracticeGoal, PracticeSession, ReviewAttempt, ReviewCard, Square } from "../api/types";
// 单元测试直接用 Node 运行这个文件：值导入写明扩展名
import { entryState, GOAL_LABEL, outcomeOf, practiceAttempts, type AttemptOutcome, type Tone } from "./reviewList.ts";
import { sideLabel } from "../utils/board.ts";

type Card = Pick<ReviewCard, "kind" | "side" | "targets" | "answer" | "played">;
type Attempt = Pick<ReviewAttempt, "choice" | "marks" | "matched">;

/** 作答方式：board 在棋盘上走子；pick 在棋盘上点出格子后提交；options 在两个选项中选一个 */
export type AnswerMode = "board" | "pick" | "options";

interface KindSpec {
  label: string;
  answer: AnswerMode;
  /** 题型决定的判断方式；为空时按用户选择的练习目标 */
  goal: PracticeGoal | null;
  /** 作答前的说明（side 是“红方”“黑方”） */
  prompt: (card: Card, side: string) => string;
  /** 不作答、直接看答案的按钮 */
  reveal: string;
  /** 专注练习顶部：这道题看什么（goal 为空时按练习目标说明） */
  judge: string;
  /** 揭示后“具体原因”中答案的标题 */
  answerTitle: (card: Card) => string;
  /** 揭示后棋盘上的标记说明 */
  legend: (withChoice: boolean) => string;
  /** 复现棋谱一类判断的结果标签：答对、没有答对 */
  recall: [string, string];
  /** 最近几次复习的记号说明 */
  marks: string;
  /** 揭示后的结果说明 */
  result: (card: Card, a: Attempt) => string;
  /** 局面决策的结论中与默认说法不同的几种 */
  verdicts?: Partial<Record<DecisionVerdict, string>>;
  /** 局面决策没有建议状态时，提醒自己检查理解 */
  selfCheck?: string;
}

const ENGINE_LEGEND = (withChoice: boolean) => `棋盘上实线箭头是引擎首选${withChoice ? "，虚线是这次的选择" : ""}。`;
const DECISION_MARKS = "○ 比较支持这个选择 △ 不支持 · 无法判断";
const SELF_CHECK_TAIL = "不等于已经理解：对照原来的意图与这一步实际的变化，说得清理由再标为“已理解”。";

/** 生成的题目（二选一、找更好的着法）揭示后的结果：对照引擎首选与当时走的着法 */
function engineResult(card: Card, a: Attempt): string {
  const played = card.played?.notation ?? "";
  if (!a.choice) return `这次没有选择，直接查看了答案：引擎首选是 ${card.answer.notation}，当时走了 ${played}。`;
  if (a.choice.notation === card.answer.notation) return `这次选了 ${a.choice.notation}，正是引擎首选（当时走了 ${played}）。`;
  if (a.choice.notation === played) return `这次选了 ${played}，与当时一样；引擎首选是 ${card.answer.notation}。`;
  return `这次走了 ${a.choice.notation}；引擎首选是 ${card.answer.notation}，当时走了 ${played}。`;
}

export const KINDS: Record<CardKind, KindSpec> = {
  move: {
    label: "棋谱中的一着",
    answer: "board",
    goal: null,
    prompt: (_, side) => `轮到${side}走。在棋盘上走出你的选择，再对照原来的思考与讲解。`,
    reveal: "不走子，直接看答案",
    judge: "",
    answerTitle: () => "棋谱中的这一着",
    legend: (withChoice) => `棋盘上实线箭头是棋谱中的着法${withChoice ? "，虚线是这次的选择" : ""}。`,
    recall: ["与棋谱一致", "与棋谱不同"],
    marks: `（复现棋谱 ✓ 一致 ✗ 不同；局面决策 ${DECISION_MARKS}）`,
    result: (card, a) => {
      const answer = card.answer.notation;
      if (!a.choice) return `这次没有走子，直接查看了答案：棋谱中是 ${answer}。`;
      return a.matched ? `这次走了 ${a.choice.notation}，与棋谱相同。` : `这次走了 ${a.choice.notation}，棋谱中是 ${answer}。`;
    },
    selfCheck: `与棋谱一致或评分接近，${SELF_CHECK_TAIL}`,
  },
  threat: {
    label: "找威胁",
    answer: "pick",
    goal: "recall",
    prompt: (_, side) => `轮到${side}走。先不走子：在棋盘上点出受到威胁的己方棋子（被将军时还要点出将军的棋子），再提交。`,
    reveal: "直接看答案",
    judge: "看是否点出了全部受到威胁的棋子（按规则判断）",
    answerTitle: (card) => `受到威胁的位置：${card.targets.join("、")}；当时走的着法`,
    legend: () => "棋盘上圈出的是受到威胁的位置，高亮的是你点出的格子。",
    recall: ["全部找到", "没有全部找到"],
    marks: "（✓ 全部找到 ✗ 没有全部找到）",
    result: (card, a) => {
      if (!a.marks?.length) return `这次没有点出格子，直接查看了答案：受到威胁的有 ${card.targets.length} 处。`;
      if (a.matched) return `点出了全部 ${card.targets.length} 处受到威胁的棋子。`;
      const found = a.marks.filter((m) => card.targets.includes(m)).length;
      const extra = a.marks.length - found;
      return `找到 ${found} / ${card.targets.length} 处${extra ? `，多点了 ${extra} 处` : ""}。`;
    },
  },
  choice: {
    label: "二选一",
    answer: "options",
    goal: "recall",
    prompt: (_, side) => `轮到${side}走。下面两步中哪一步更好？选好之后对照同一次搜索的结论。`,
    reveal: "不选，直接看答案",
    judge: "看是否选了同一次搜索中更好的一步",
    answerTitle: () => "答案",
    legend: ENGINE_LEGEND,
    recall: ["选对了", "没有选对"],
    marks: "（✓ 选对了 ✗ 没有选对）",
    result: engineResult,
  },
  better: {
    label: "找更好的着法",
    answer: "board",
    goal: "decision",
    prompt: (card, side) =>
      `轮到${side}走。当时走了 ${card.played?.notation ?? ""}，程序评价为明显不如引擎首选。在棋盘上走出你认为更好的一步。`,
    reveal: "不走子，直接看答案",
    judge: "看这次的选择是否得到同一次搜索比较的支持",
    answerTitle: () => "答案",
    legend: ENGINE_LEGEND,
    recall: ["与引擎首选一致", "与引擎首选不同"],
    marks: `（${DECISION_MARKS} ✗ 直接看了答案）`,
    result: engineResult,
    // 这类题目的“棋谱着法”是出题时的引擎首选，不是当时走的着法
    verdicts: { same_as_record: "与题目中的引擎首选一致", reasonable: "与题目中的引擎首选不同，但本次分析中与参考评分接近" },
    selfCheck: `选了引擎首选或评分接近，${SELF_CHECK_TAIL}`,
  },
};

export const KIND_LABEL: Record<CardKind, string> = Object.fromEntries(Object.entries(KINDS).map(([k, s]) => [k, s.label])) as Record<CardKind, string>;

/** 练习目标的说明：选择目标时（棋谱中的一着）与专注练习顶部 */
export const GOAL_HINT: Record<PracticeGoal, string> = {
  recall: "回忆这盘棋当时怎样走，看是否与棋谱一致",
  decision: "作出自己的选择，与棋谱着法、引擎参考在同一次搜索中比较",
};
const GOAL_JUDGE: Record<PracticeGoal, string> = { recall: "看是否记得棋谱中的着法", decision: "看这次的选择是否得到同一次搜索比较的支持" };

/** 专注练习顶部的说明：题型决定判断方式时写题型，否则写这组练习的目标 */
export function judgeText(kind: CardKind, goal: PracticeGoal): string {
  const spec = KINDS[kind];
  return spec.goal === null ? `${GOAL_LABEL[goal]}：${GOAL_JUDGE[goal]}` : `${spec.label}：${spec.judge}`;
}

export const promptText = (card: Card) => KINDS[card.kind].prompt(card, sideLabel(card.side));

/** 揭示后的结果说明；还没有找到这次作答时按直接看答案说明 */
export const resultText = (card: Card, a: Attempt | null) => KINDS[card.kind].result(card, a ?? { choice: null, marks: [], matched: false });

/** 结果说明的颜色：答对为 ok；局面决策中与答案不同不等于走错，是中性的；其余为 miss */
export function resultTone(a: Pick<ReviewAttempt, "matched" | "goal"> | null): Tone {
  if (a?.matched) return "ok";
  return a?.goal === "decision" ? "neutral" : "miss";
}

const DECISION_BADGE: Record<Exclude<AttemptOutcome, "revealed" | "assisted">, { text: string; tone: Tone }> = {
  independent: { text: "比较支持", tone: "ok" },
  missed: { text: "比较不支持", tone: "miss" },
  pending: { text: "比较中", tone: "neutral" },
  ungraded: { text: "无法评价", tone: "neutral" },
};

/**
 * 一次作答的结果标签：练习中的列表、反馈与小结用同样的文字与颜色。复现棋谱一类按是否答对；
 * 局面决策按这次比较是否支持这个选择，比较中与无法评价都是中性的，不当作选对或选错。
 */
export function outcomeBadge(a: ReviewAttempt, kind: CardKind = "move"): { text: string; tone: Tone } {
  const o = outcomeOf(a);
  if (o === "revealed") return { text: "直接看了答案", tone: "neutral" };
  if (o === "assisted") return { text: "看了提示后选对", tone: "neutral" };
  if (a.goal === "recall") {
    const [ok, miss] = KINDS[kind].recall;
    return o === "independent" ? { text: ok, tone: "ok" } : { text: miss, tone: "miss" };
  }
  return DECISION_BADGE[o];
}

/** 一组练习中一道题的标签：作答过的按这组中的第一次作答 */
export function entryBadge(session: Pick<PracticeSession, "id" | "skipped">, cardId: string, card: ReviewCard | undefined): { text: string; tone: Tone } {
  const state = entryState(session, cardId, card);
  if (state === "answered" && card) return outcomeBadge(practiceAttempts(card, session.id)[0], card.kind);
  return { text: state === "missing" ? "卡片已删除" : state === "skipped" ? "已跳过" : "未作答", tone: "neutral" };
}

export const VERDICT_TEXT: Record<DecisionVerdict, string> = {
  same_as_record: "与棋谱一致",
  reasonable: "与棋谱不同，但本次分析中与参考评分接近",
  noteworthy: "这个选择留下了一个值得注意的问题",
  unclear: "分析尚不充分，暂不评价走法优劣",
};

/** 局面决策反馈的标题（比较还在进行或没有得到结果时说明原因）。 */
export function decisionTitle(d: DecisionFeedback, kind: CardKind = "move"): string {
  if (d.state === "pending") return "正在用同一次搜索比较你的选择、棋谱着法与引擎参考…";
  if (d.state === "failed" || !d.verdict) return `没有得到比较结果：${d.error ?? "原因不明"}`;
  return KINDS[kind].verdicts?.[d.verdict] ?? VERDICT_TEXT[d.verdict];
}

/** 最近几次复习中一次的说明（鼠标悬停时显示）：判断方式与比较结论 */
export function attemptTitle(kind: CardKind, a: ReviewAttempt): string {
  const how = KINDS[kind].goal === null ? GOAL_LABEL[a.goal] : KINDS[kind].label;
  return a.decision?.verdict ? `${how} · ${decisionTitle(a.decision, kind)}` : how;
}

export interface BoardLayers {
  arrows: Move[];
  marks: Square[];
  highlights: Square[];
  /** 揭示后这次的选择（与答案不同时，虚线） */
  next: Move | null;
}

/**
 * 棋盘上的标记。选择前：点格子作答时是点出的格子，其他题型是二级提示标出的棋子与关系；
 * 揭示后：点格子作答时圈出答案的格子、高亮自己点出的，其他题型实线是答案，虚线是这次与答案不同的选择。
 */
export function boardLayers(
  card: Card,
  revealed: boolean,
  s: { marks: Square[]; hintMarks: Square[]; hintArrows: Move[]; attempt: Pick<ReviewAttempt, "choice" | "matched" | "marks"> | null },
): BoardLayers {
  const pick = KINDS[card.kind].answer === "pick";
  if (!revealed) return pick ? { arrows: [], marks: s.marks, highlights: [], next: null } : { arrows: s.hintArrows, marks: s.hintMarks, highlights: [], next: null };
  if (pick) return { arrows: [], marks: card.targets, highlights: s.attempt?.marks ?? [], next: null };
  const a = s.attempt;
  return { arrows: [card.answer.mv], marks: [], highlights: [], next: a?.choice && !a.matched ? a.choice.mv : null };
}
