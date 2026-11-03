// 复习卡片的分组、计数与文字：列表不透露答案，复习后的文字说明这次与原来的思考。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { ArchiveSummary, DecisionFeedback, ReviewAttempt, ReviewCard, ReviewIssue, ReviewItem, ReviewStatus, Theme } from "../../src/api/types.ts";
import {
  archivePreviewLines,
  attemptMark,
  attemptNotesText,
  counts,
  DAY_MS,
  dueText,
  filterItems,
  hasTopic,
  historyText,
  awaitingResult,
  newerItem,
  nextDueId,
  outcomeOf,
  parseIntervals,
  positionLabel,
  recommendation,
  reviewCardId,
  suggestedStatus,
  entryState,
  practiceSummary,
  presentFrom,
  suggestionFor,
  summaryText,
  thinkingText,
  topicLabel,
  topics,
  whenText,
} from "../../src/state/reviewList.ts";
import { decisionTitle, entryBadge, outcomeBadge, resultText } from "../../src/state/cardKind.ts";

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

test("夏令时结束的那天有 25 小时：当天最后一小时加入的卡片仍是今天到期", () => {
  const tz = process.env.TZ;
  process.env.TZ = "America/Los_Angeles";
  try {
    // 2026-11-01 洛杉矶 02:00 时钟拨回 01:00；23:30 时零点加 24 小时已经是 23:00
    const now = Date.parse("2026-11-01T23:30:00-08:00");
    const card = (at: string) => ({ ...item("a", "due").card, due_at: Date.parse(at) });
    assert.equal(filterItems([{ ...item("a", "due"), card: card("2026-11-01T23:25:00-08:00") }], "today", now).length, 1);
    assert.equal(filterItems([{ ...item("a", "due"), card: card("2026-11-02T00:10:00-08:00") }], "today", now).length, 0, "明天的不算");
    assert.equal(dueText(card("2026-11-02T09:00:00-08:00"), now), "明天再看");
  } finally {
    if (tz === undefined) delete process.env.TZ;
    else process.env.TZ = tz;
  }
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
  const moveCard = { kind: "move" as const, side: "black" as const, targets: [], answer: { mv: "h9g7", notation: "马8进7" }, played: null };
  assert.equal(resultText(moveCard, { choice: first, marks: [], matched: false }), "这次走了 马2进3，棋谱中是 马8进7。");
  assert.equal(resultText(moveCard, { choice: moveCard.answer, marks: [], matched: true }), "这次走了 马8进7，与棋谱相同。");
  assert.match(resultText(moveCard, null), /直接查看了答案/);
  assert.equal(suggestedStatus(true), "understood");
  assert.equal(suggestedStatus(false), "again");
});

test("下一张今天到期的卡片：跳过还没到期的，到末尾从头找", () => {
  const items = [item("a", "due"), item("b", "understood", [], true, 3), item("c", "again")];
  assert.equal(nextDueId(items, "a", NOW), "c");
  assert.equal(nextDueId(items, "c", NOW), "a");
  assert.equal(nextDueId([item("a", "due")], "a", NOW), null);
});

test("一组练习的小结只统计属于这组练习的作答，按第一次作答计算，安排按时间先后", () => {
  const P = "p1";
  const attempt = (at: number, choice: boolean, matched: boolean, practice: string | null = P): ReviewAttempt => ({
    at,
    choice: choice ? { mv: "h9g7", notation: "马8进7" } : null,
    matched,
    goal: "recall",
    intent: null,
    reflection: null,
    decision: null,
    practice,
    request: null,
  });
  const card = (id: string, days: number, attempts: ReviewAttempt[]) => ({ ...item(id, "due", [], true, days).card, attempts });
  const session = (ids: string[], skipped: string[] = []) => ({ id: P, cards: ids, skipped });
  const cards = [
    card("a", 3, [attempt(NOW, true, true)]),
    // 这组之外的复习不算；这组中第一次没有选对，揭示后再试一次选对：按第一次计算
    card("b", 1, [attempt(NOW - 9, true, true, null), attempt(NOW, true, false), attempt(NOW + 1, true, true)]),
    card("c", 1, [attempt(NOW, false, false)]),
    // 这组里没有作答
    card("d", 0, [attempt(NOW - 5, true, true, "另一组")]),
  ];
  const s = practiceSummary(session(["a", "b", "c", "d"]), cards, NOW);
  assert.deepEqual(
    { ...s, schedule: undefined },
    { total: 4, done: 3, independent: 1, assisted: 0, missed: 1, revealed: 1, pending: 0, ungraded: 0, skipped: 0, missing: 0, schedule: undefined },
  );
  assert.deepEqual(s.schedule, [
    { text: "明天再看", count: 2 },
    { text: "3 天后再看", count: 1 },
  ]);
  assert.equal(summaryText(s), "本次完成 3 张：1 张独立选对，1 张没有选对，1 张直接看了答案；还有 1 张没有复习。安排：明天再看 2 张，3 天后再看 1 张。");
  assert.equal(summaryText(practiceSummary(session(["d"]), cards, NOW)), "这组 1 张还没有复习。");
  // 跳过与已删除的卡片分开说明；跳过之后又作答的按作答计算
  const withGaps = practiceSummary(session(["a", "d", "x"], ["d", "a"]), cards, NOW);
  assert.deepEqual([withGaps.done, withGaps.skipped, withGaps.missing], [1, 1, 1]);
  assert.equal(summaryText(withGaps), "本次完成 1 张：1 张独立选对；跳过 1 张；1 张卡片已删除。安排：3 天后再看 1 张。");
  assert.deepEqual(
    ["a", "d", "x"].map((id) => entryState(session(["a", "d", "x"], ["d"]), id, cards.find((c) => c.id === id))),
    ["answered", "skipped", "missing"],
  );
  assert.equal(entryState(session(["d"]), "d", cards[3]), "todo");
  // 练习中的标签：作答过的按这组中的第一次作答，其余写明进展
  const gaps = session(["b", "c", "d", "x"], ["d"]);
  assert.deepEqual(
    ["b", "c", "d", "x"].map((id) => entryBadge(gaps, id, cards.find((c) => c.id === id)).text),
    ["与棋谱不同", "直接看了答案", "已跳过", "卡片已删除"],
  );
  // 往后找卡片还在的题目
  const exists = (id: string) => id !== "x";
  assert.equal(presentFrom({ cards: ["a", "x", "b"] }, 1, exists), 2);
  assert.equal(presentFrom({ cards: ["a", "x"] }, 1, exists), null);

  // 局面决策：比较还没有结果、无法评价，都不折算为选对或选错；已经标记、等结果的卡片安排另外说明
  const decision = (state: "pending" | "ready", support: "supported" | "unknown"): ReviewAttempt => ({
    ...attempt(NOW, true, false),
    goal: "decision",
    decision: { state, verdict: state === "ready" ? "unclear" : null, support, reference: null, points: [], conditions: null, error: null },
  });
  const waiting = { ...card("e", 1, [decision("pending", "unknown")]), schedule_base: { attempt_at: NOW, stage: 0, at: NOW } };
  const unclear = card("f", 1, [decision("ready", "unknown")]);
  assert.equal(outcomeOf(waiting.attempts[0]), "pending");
  assert.equal(outcomeOf(unclear.attempts[0]), "ungraded");
  assert.ok(awaitingResult(waiting) && !awaitingResult(unclear));
  const mixed = practiceSummary(session(["e", "f"]), [waiting, unclear], NOW);
  assert.equal(summaryText(mixed), "本次完成 2 张：1 张的比较还没有完成，1 张无法评价。安排：明天再看 1 张，比较完成后确定 1 张。");
  // 局面决策的标签：比较中与无法评价是中性的
  assert.deepEqual(outcomeBadge(waiting.attempts[0]), { text: "比较中", tone: "neutral" });
  assert.deepEqual(outcomeBadge(unclear.attempts[0]), { text: "无法评价", tone: "neutral" });
  // 结果回来、支持这个选择：小结随卡片更新
  const supported = { ...waiting, attempts: [{ ...decision("ready", "supported"), verdict: "reasonable" as const }] };
  assert.equal(practiceSummary(session(["e", "f"]), [supported, unclear], NOW).independent, 1);
  assert.deepEqual(outcomeBadge(supported.attempts[0]), { text: "比较支持", tone: "ok" });

  // 看了提示之后选对：不算独立选对，标签与小结单独说明
  const hinted = card("g", 1, [{ ...attempt(NOW, true, true), hints: 2, confidence: "unsure", elapsed_ms: 75_000 }]);
  assert.equal(outcomeOf(hinted.attempts[0]), "assisted");
  assert.deepEqual(outcomeBadge(hinted.attempts[0]), { text: "看了提示后选对", tone: "neutral" });
  assert.equal(attemptNotesText(hinted.attempts[0]), "看了 2 级提示 · 选择时有些把握 · 用时 1 分 15 秒");
  assert.equal(attemptNotesText({ hints: 0, confidence: null, elapsed_ms: null }), "");
  const withHints = practiceSummary(session(["a", "g"]), [cards[0], hinted], NOW);
  assert.deepEqual([withHints.independent, withHints.assisted], [1, 1]);
  assert.ok(summaryText(withHints).includes("1 张看了提示后选对"), summaryText(withHints));
  const hintedDecision = { ...decision("ready", "supported"), hints: 1 };
  assert.equal(outcomeOf(hintedDecision), "assisted");
  // 生成的题目：找威胁按点出的格子，二选一按选项；结果文字不说“棋谱中”
  const threatCard = { kind: "threat" as const, side: "red" as const, targets: ["e7", "e2"], answer: { mv: "e2c3", notation: "马五进七" }, played: null };
  const picked = { ...attempt(NOW, false, false), choice: null, marks: ["e7", "a0"] };
  assert.equal(outcomeOf(picked), "missed", "点出了格子就不是直接看答案");
  assert.equal(resultText(threatCard, picked), "找到 1 / 2 处，多点了 1 处。");
  assert.equal(resultText(threatCard, { ...picked, marks: ["e7", "e2"], matched: true }), "点出了全部 2 处受到威胁的棋子。");
  assert.equal(resultText(threatCard, { ...picked, marks: [] }), "这次没有点出格子，直接查看了答案：受到威胁的有 2 处。");
  assert.deepEqual(outcomeBadge({ ...picked, matched: true }, "threat"), { text: "全部找到", tone: "ok" });
  const choiceCard = { kind: "choice" as const, side: "red" as const, targets: [], answer: { mv: "e2g3", notation: "马五进三" }, played: { mv: "e2c3", notation: "马五进七" } };
  const same = { ...attempt(NOW, true, false), choice: { mv: "e2c3", notation: "马五进七" } };
  assert.equal(resultText(choiceCard, same), "这次选了 马五进七，与当时一样；引擎首选是 马五进三。");
  assert.deepEqual(outcomeBadge(same, "choice"), { text: "没有选对", tone: "miss" });
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

test("局面决策：按这次比较是否支持这个选择建议状态，与棋谱一致不自动建议已理解", () => {
  const base = { at: 1, choice: { mv: "b9c7", notation: "马2进3" }, matched: false, intent: null, reflection: null, decision: null, practice: null, request: null };
  const SUPPORT = { same_as_record: "supported", reasonable: "supported", noteworthy: "not_supported", unclear: "unknown" } as const;
  const decided = (
    verdict: DecisionFeedback["verdict"],
    state: DecisionFeedback["state"] = "ready",
    support: DecisionFeedback["support"] = verdict ? SUPPORT[verdict] : "unknown",
  ): ReviewAttempt => ({
    ...base,
    goal: "decision",
    decision: { state, verdict, support, reference: null, points: [], conditions: null, error: state === "failed" ? "引擎不可用" : null },
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
  // 与棋谱一致，但棋谱着法明显不如引擎参考：仍显示“与棋谱一致”，但不算选对，建议再练
  const knownBad = { ...decided("same_as_record", "ready", "not_supported"), matched: true };
  assert.equal(decisionTitle(knownBad.decision!), "与棋谱一致");
  assert.equal(suggestionFor(knownBad), "again");
  assert.deepEqual(attemptMark(knownBad), { text: "△", tone: "miss" });
  assert.equal(outcomeOf(knownBad), "missed");
});

test("专题：用户确认的问题在前，推荐只按同一个问题标记到两张以上", () => {
  const withTopics = (id: string, themes: Theme[], issues: ReviewIssue[]): ReviewItem => {
    const base = item(id, "due");
    return { ...base, card: { ...base.card, issues }, themes: themes.map((theme) => ({ theme, why: "依据" })) };
  };
  const items = [
    withTopics("a", ["protection", "recapture"], ["missed_protection"]),
    withTopics("b", ["protection"], ["missed_protection", "missed_reply"]),
    withTopics("c", ["lines"], []),
  ];
  assert.deepEqual(
    topics(items).map((t) => `${t.label}×${t.count}`),
    // 张数相同时按主题的固定顺序
    ["我标记的：没有注意保护关系×2", "我标记的：没有考虑对方的回应×1", "保护关系变化×2", "炮架与线路变化×1", "吃子后的回吃关系×1"],
  );
  assert.ok(hasTopic(items[0], "theme:recapture") && !hasTopic(items[2], "issue:missed_reply"));
  assert.equal(topicLabel("theme:king_safety"), "应将与将帅安全");
  assert.deepEqual(recommendation(items), { key: "issue:missed_protection", issue: "missed_protection", count: 2 });
  assert.equal(recommendation(items.slice(1)), null, "只标记一张时不推荐");
});
