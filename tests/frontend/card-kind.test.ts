// 题型：作答方式、说明、棋盘标记、结果标签与比较结论都按题型取，生成的题目不说“棋谱中”。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { CardKind, DecisionFeedback, ReviewAttempt } from "../../src/api/types.ts";
import { attemptTitle, boardLayers, decisionTitle, judgeText, KIND_LABEL, KINDS, outcomeBadge, promptText, resultText, resultTone } from "../../src/state/cardKind.ts";

const move = (mv: string, notation: string) => ({ mv, notation });
const cards = {
  move: { kind: "move" as const, side: "black" as const, targets: [], answer: move("h9g7", "马8进7"), played: null },
  threat: { kind: "threat" as const, side: "red" as const, targets: ["e7"], answer: move("e2c3", "马五进七"), played: null },
  choice: { kind: "choice" as const, side: "red" as const, targets: [], answer: move("e2g3", "马五进三"), played: move("e2c3", "马五进七") },
  better: { kind: "better" as const, side: "red" as const, targets: [], answer: move("e2g3", "马五进三"), played: move("e2c3", "马五进七") },
};

const attempt = (over: Partial<ReviewAttempt>): ReviewAttempt => ({
  at: 1,
  choice: null,
  matched: false,
  goal: "recall",
  intent: null,
  reflection: null,
  decision: null,
  practice: null,
  request: null,
  hints: 0,
  elapsed_ms: null,
  confidence: null,
  marks: [],
  ...over,
});
const decided = (verdict: DecisionFeedback["verdict"], support: DecisionFeedback["support"]): DecisionFeedback => ({
  state: "ready",
  verdict,
  support,
  reference: null,
  points: [],
  conditions: null,
  error: null,
  stability: null,
});

test("作答方式与判断方式和后台一致", () => {
  const modes = Object.fromEntries((Object.keys(KINDS) as CardKind[]).map((k) => [k, [KINDS[k].answer, KINDS[k].goal]]));
  // 后台 review_attempt：找威胁与二选一记为复现棋谱，找更好的着法总是局面决策，棋谱中的一着按用户选择
  assert.deepEqual(modes, { move: ["board", null], threat: ["pick", "recall"], choice: ["options", "recall"], better: ["board", "decision"] });
  assert.deepEqual(KIND_LABEL, { move: "棋谱中的一着", threat: "找威胁", choice: "二选一", better: "找更好的着法" });
  assert.equal(KINDS.choice.reveal, "不选，直接看答案", "二选一不走子");
});

test("作答前的说明与专注练习顶部的判断方式", () => {
  assert.equal(promptText(cards.move), "轮到黑方走。在棋盘上走出你的选择，再对照原来的思考与讲解。");
  assert.match(promptText(cards.threat), /^轮到红方走。先不走子：在棋盘上点出受到威胁的己方棋子/);
  assert.match(promptText(cards.better), /当时走了 马五进七，程序评价为明显不如引擎首选/);
  // 棋谱中的一着按这组练习的目标；生成的题目按题型，不随练习目标改变
  assert.equal(judgeText("move", "recall"), "复现棋谱：看是否记得棋谱中的着法");
  assert.equal(judgeText("move", "decision"), "局面决策：看这次的选择是否得到同一次搜索比较的支持");
  assert.equal(judgeText("threat", "decision"), "找威胁：看是否点出了全部受到威胁的棋子（按规则判断）");
  assert.equal(judgeText("choice", "decision"), "二选一：看是否选了同一次搜索中更好的一步");
  assert.ok(judgeText("better", "recall").startsWith("找更好的着法："));
});

test("棋盘上的标记：点格子作答与走子作答", () => {
  const hints = { marks: ["a0"], hintMarks: ["e2"], hintArrows: ["e2c3"], attempt: null };
  // 选择前：找威胁显示点出的格子，不显示二级提示；其他题型显示二级提示
  assert.deepEqual(boardLayers(cards.threat, false, hints), { arrows: [], marks: ["a0"], highlights: [], next: null });
  assert.deepEqual(boardLayers(cards.move, false, hints), { arrows: ["e2c3"], marks: ["e2"], highlights: [], next: null });
  // 揭示后：找威胁圈出答案、高亮自己点出的；其他题型实线是答案，与答案不同的选择是虚线
  const picked = { ...hints, attempt: attempt({ marks: ["e7", "a0"] }) };
  assert.deepEqual(boardLayers(cards.threat, true, picked), { arrows: [], marks: ["e7"], highlights: ["e7", "a0"], next: null });
  const wrong = { ...hints, attempt: attempt({ choice: move("e2c3", "马五进七") }) };
  assert.deepEqual(boardLayers(cards.choice, true, wrong), { arrows: ["e2g3"], marks: [], highlights: [], next: "e2c3" });
  const right = { ...hints, attempt: attempt({ choice: move("h9g7", "马8进7"), matched: true }) };
  assert.equal(boardLayers(cards.move, true, right).next, null, "选对时只有答案的箭头");
  assert.equal(KINDS.move.legend(true), "棋盘上实线箭头是棋谱中的着法，虚线是这次的选择。");
  assert.equal(KINDS.better.legend(false), "棋盘上实线箭头是引擎首选。");
});

test("结果说明、标签与颜色按题型", () => {
  assert.equal(resultText(cards.better, attempt({ choice: move("e2g3", "马五进三"), matched: true })), "这次选了 马五进三，正是引擎首选（当时走了 马五进七）。");
  assert.equal(resultText(cards.threat, null), "这次没有点出格子，直接查看了答案：受到威胁的有 1 处。");
  assert.deepEqual(outcomeBadge(attempt({ choice: move("e2g3", "马五进三"), matched: true }), "choice"), { text: "选对了", tone: "ok" });
  assert.deepEqual(outcomeBadge(attempt({ marks: ["a0"] }), "threat"), { text: "没有全部找到", tone: "miss" });
  assert.deepEqual(outcomeBadge(attempt({ choice: move("h9h0", "车9平8") }), "move"), { text: "与棋谱不同", tone: "miss" });
  // 找更好的着法按比较是否支持
  const better = attempt({ goal: "decision", choice: move("e2e1", "马五退六"), decision: decided("noteworthy", "not_supported") });
  assert.deepEqual(outcomeBadge(better, "better"), { text: "比较不支持", tone: "miss" });
  assert.equal(resultTone(attempt({ matched: true })), "ok");
  assert.equal(resultTone(better), "neutral", "局面决策中与答案不同不等于走错");
  assert.equal(resultTone(attempt({})), "miss");
});

test("找更好的着法的比较结论不说“与棋谱一致”", () => {
  const same = decided("same_as_record", "supported");
  assert.equal(decisionTitle(same), "与棋谱一致");
  assert.equal(decisionTitle(same, "better"), "与题目中的引擎首选一致");
  assert.equal(decisionTitle(decided("reasonable", "supported"), "better"), "与题目中的引擎首选不同，但本次分析中与参考评分接近");
  assert.equal(decisionTitle(decided("noteworthy", "not_supported"), "better"), "这个选择留下了一个值得注意的问题");
  // 最近几次的说明：棋谱中的一着写练习目标，生成的题目写题型
  assert.equal(attemptTitle("move", attempt({ goal: "decision", decision: same })), "局面决策 · 与棋谱一致");
  assert.equal(attemptTitle("better", attempt({ goal: "decision", decision: same })), "找更好的着法 · 与题目中的引擎首选一致");
  assert.equal(attemptTitle("threat", attempt({})), "找威胁");
  assert.ok(!KINDS.threat.marks.includes("复现棋谱") && !KINDS.better.marks.includes("复现棋谱"), "记号说明不提生成的题目用不到的练习目标");
  assert.ok(KINDS.better.selfCheck?.startsWith("选了引擎首选"));
  assert.equal(KINDS.threat.selfCheck, undefined, "按是否答对判断的题型不需要自我检查的提醒");
});
