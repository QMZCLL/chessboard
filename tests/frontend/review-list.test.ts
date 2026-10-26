// 复习卡片的分组、计数与文字：列表不透露答案，复习后的文字说明这次与原来的思考。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { ArchiveSummary, DecisionFeedback, ReviewAttempt, ReviewCard, ReviewItem, ReviewStatus } from "../../src/api/types.ts";
import {
  archivePreviewLines,
  attemptMark,
  attemptText,
  counts,
  DAY_MS,
  decisionTitle,
  dueText,
  filterItems,
  historyText,
  newerItem,
  nextDueId,
  parseIntervals,
  positionLabel,
  reviewCardId,
  suggestedStatus,
  sessionSummary,
  suggestionFor,
  summaryText,
  thinkingText,
  whenText,
} from "../../src/state/reviewList.ts";

/** 本机时间 2026-10-25 20:00 */
const NOW = new Date(2026, 9, 25, 20, 0).getTime();

function item(id: string, status: ReviewStatus, history: string[] = [], complete = true, dueInDays = 0): ReviewItem {
  const card = {
    id,
    status,
    due_at: NOW + dueInDays * DAY_MS,
    created_at: 0,
    turn: 3,
    side: "black",
    history: history.map((notation) => ({ mv: "a0a1", notation })),
    history_complete: complete,
    answer: { mv: "h9g7", notation: "马8进7" },
  } as unknown as ReviewCard;
  return { card, source_available: true, study_name: "测试" };
}

test("今天到期按下一次复习的时间（含已经过期的），需要再练与已理解按状态分组", () => {
  const items = [item("a", "again", [], true, 1), item("b", "due", [], true, -2), item("c", "understood", [], true, 0), item("d", "understood", [], true, 3)];
  assert.deepEqual(filterItems(items, "today", NOW).map((i) => i.card.id), ["b", "c"]);
  assert.deepEqual(filterItems(items, "again", NOW).map((i) => i.card.id), ["a"]);
  assert.deepEqual(filterItems(items, "understood", NOW).map((i) => i.card.id), ["c", "d"]);
  assert.deepEqual(counts(items, NOW), { today: 2, again: 1, understood: 2, all: 4 });
  // 今天稍晚的时间也算今天到期（按本机日期）
  const tonight = item("e", "due");
  tonight.card.due_at = new Date(2026, 9, 25, 23, 30).getTime();
  assert.equal(filterItems([tonight], "today", NOW).length, 1);
});

test("下一次复习的说明按本机日期", () => {
  const at = (days: number) => item("a", "due", [], true, days).card;
  assert.equal(dueText(at(0), NOW), "今天到期");
  assert.equal(dueText(at(-2), NOW), "已到期 2 天");
  assert.equal(dueText(at(1), NOW), "明天再看");
  assert.equal(dueText(at(3), NOW), "3 天后再看");
  assert.equal(dueText(at(14), NOW), "11 月 8 日再看");
});

test("列表中的说明不含棋谱中的着法", () => {
  const label = positionLabel(item("a", "due").card);
  assert.equal(label, "第 3 回合 · 黑方走");
  assert.ok(!label.includes("马8进7"));
});

test("此前的着法只显示最后几着，历史不完整时说明", () => {
  assert.equal(historyText(item("a", "due", ["炮二平五", "马8进7"]).card), "此前：炮二平五 马8进7");
  const long = item("a", "due", ["1", "2", "3", "4", "5", "6", "7"]).card;
  assert.equal(historyText(long, 3), "此前：… 5 6 7");
  assert.equal(historyText(item("a", "due", [], false).card), "从这个局面开始（此前的历史未知）");
  assert.equal(historyText(item("a", "due", ["车一平二"], false).card), "此前：… 车一平二");
});

test("原来的思考与这次复习的结果", () => {
  const first = { mv: "b9c7", notation: "马2进3" };
  assert.equal(
    thinkingText({ first_choice: first, hints_used: 2, revealed_directly: false, matched: false, hints: null, practiced_at: 0 }),
    "当时用了 2 级提示，第一步走了 马2进3，与棋谱不同。",
  );
  assert.equal(
    thinkingText({ first_choice: null, hints_used: 0, revealed_directly: true, matched: false, hints: null, practiced_at: 0 }),
    "当时没有用提示，没有走子就看了讲解。",
  );
  assert.match(thinkingText(null), /没有先想后讲的记录/);
  assert.equal(attemptText(first, false, "马8进7"), "这次走了 马2进3，棋谱中是 马8进7。");
  assert.equal(attemptText({ notation: "马8进7" }, true, "马8进7"), "这次走了 马8进7，与棋谱相同。");
  assert.match(attemptText(null, false, "马8进7"), /直接查看了答案/);
  assert.equal(suggestedStatus(true), "understood");
  assert.equal(suggestedStatus(false), "again");
});

test("下一张今天到期的卡片：跳过还没到期的，到末尾从头找", () => {
  const items = [item("a", "due"), item("b", "understood", [], true, 3), item("c", "again")];
  assert.equal(nextDueId(items, "a", NOW), "c");
  assert.equal(nextDueId(items, "c", NOW), "a");
  assert.equal(nextDueId([item("a", "due")], "a", NOW), null);
});

test("一组练习的小结只统计这组开始之后的复习，安排按时间先后", () => {
  const started = NOW - 60_000;
  const attempt = (at: number, choice: boolean, matched: boolean): ReviewAttempt => ({
    at,
    choice: choice ? { mv: "h9g7", notation: "马8进7" } : null,
    matched,
    goal: "recall",
    intent: null,
    reflection: null,
    decision: null,
  });
  const card = (id: string, days: number, attempts: ReviewAttempt[]) => ({ ...item(id, "due", [], true, days).card, attempts });
  const cards = [
    card("a", 3, [attempt(NOW, true, true)]),
    card("b", 1, [attempt(started - 1, true, true), attempt(NOW, false, false)]),
    card("c", 1, [attempt(NOW, true, false)]),
    // 这组开始之前复习过、这组里没有复习
    card("d", 0, [attempt(started - 5, true, true)]),
  ];
  const s = sessionSummary(cards, started, NOW);
  assert.deepEqual({ ...s, schedule: undefined }, { total: 4, done: 3, independent: 1, missed: 1, revealed: 1, schedule: undefined });
  assert.deepEqual(s.schedule, [
    { text: "明天再看", count: 2 },
    { text: "3 天后再看", count: 1 },
  ]);
  assert.equal(summaryText(s), "本次完成 3 张：1 张独立选对，1 张没有选对，1 张直接看了答案；还有 1 张没有复习。安排：明天再看 2 张，3 天后再看 1 张。");
  assert.equal(summaryText(sessionSummary(cards.slice(3), started, NOW)), "这组 1 张还没有复习。");
});

test("复习间隔的文字与导入预览", () => {
  assert.deepEqual(parseIntervals("1, 3，7、14 30"), [1, 3, 7, 14, 30]);
  assert.deepEqual(parseIntervals("0, -2, x, 2.5"), []);
  const summary: ArchiveSummary = {
    exported_at: NOW,
    app_version: "0.2.0",
    studies_new: 2,
    studies_existing: 1,
    cards_new: 3,
    cards_same: 1,
    cards_merge: 2,
    cards_orphan: 1,
  };
  const lines = archivePreviewLines(summary);
  assert.equal(lines[0], "导出于 2026-10-25 20:00（版本 0.2.0）");
  assert.equal(lines[1], "学习会话：新增 2 个；本机已有 1 个，保留本机的版本");
  assert.equal(lines[2], "复习卡片：新增 3 张；与本机相同 1 张；需要合并 2 张");
  assert.ok(lines.some((l) => l.includes("历次复习按时间合在一起")) && lines.some((l) => l.includes("不能打开原局面")));
});

test("复习时间与卡片编号", () => {
  const now = new Date(2026, 9, 25, 20, 0).getTime();
  assert.equal(whenText(null, now), "还没有复习");
  assert.equal(whenText(new Date(2026, 9, 25, 9, 5).getTime(), now), "今天 09:05 复习过");
  assert.equal(whenText(new Date(2026, 9, 20, 9, 5).getTime(), now), "10 月 20 日复习过");
  assert.equal(reviewCardId("s1", 12), "s1/0000000012");
});

test("同一张卡片的两份数据：修改次数多的较新，较早读取的数据不覆盖它", () => {
  const at = (revision: number, note: string, updated_at = 100): ReviewItem => {
    const base = item("a", "due");
    return { ...base, card: { ...base.card, revision, note, updated_at } };
  };
  const saved = at(3, "刚保存的理解");
  const stale = at(2, "保存之前的理解");
  assert.equal(newerItem(saved, stale), saved);
  assert.equal(newerItem(stale, saved), saved);
  assert.equal(newerItem(undefined, stale), stale);
  // 同一毫秒内的两次修改：更新时间相同，仍按修改次数区分
  assert.equal(newerItem(at(5, "状态", 200), at(4, "笔记", 200)).card.note, "状态");
  // 次数相同时内容相同，采用后到的（原会话的名称等可能已经改变）
  const renamed = { ...saved, study_name: "改名后的会话" };
  assert.equal(newerItem(saved, renamed), renamed);
});

test("局面决策：只在留下值得注意的问题时建议再练，与棋谱一致不自动建议已理解", () => {
  const base = { at: 1, choice: { mv: "b9c7", notation: "马2进3" }, matched: false, intent: null, reflection: null, decision: null };
  const decided = (verdict: DecisionFeedback["verdict"], state: DecisionFeedback["state"] = "ready"): ReviewAttempt => ({
    ...base,
    goal: "decision",
    decision: { state, verdict, reference: null, points: [], conditions: null, error: state === "failed" ? "引擎不可用" : null },
  });
  // 复现棋谱沿用是否与棋谱一致
  assert.equal(suggestionFor({ ...base, goal: "recall", matched: true }), "understood");
  assert.equal(suggestionFor({ ...base, goal: "recall" }), "again");
  assert.equal(suggestionFor(decided("noteworthy")), "again");
  for (const v of ["same_as_record", "reasonable", "unclear"] as const) assert.equal(suggestionFor(decided(v)), null, v);
  assert.equal(suggestionFor(decided(null, "pending")), null);
  // 标题：进行中、没有结果、四种结论
  assert.match(decisionTitle(decided(null, "pending").decision!), /正在用同一次搜索比较/);
  assert.equal(decisionTitle(decided(null, "failed").decision!), "没有得到比较结果：引擎不可用");
  assert.equal(decisionTitle(decided("reasonable").decision!), "与棋谱不同，但本次分析中与参考评分接近");
  // 最近几次的记号
  assert.deepEqual(attemptMark({ ...base, goal: "recall", matched: true }), { text: "✓", tone: "ok" });
  assert.deepEqual(attemptMark(decided("reasonable")), { text: "○", tone: "ok" });
  assert.deepEqual(attemptMark(decided("noteworthy")), { text: "△", tone: "miss" });
  assert.deepEqual(attemptMark(decided(null, "pending")), { text: "·", tone: "neutral" });
});
