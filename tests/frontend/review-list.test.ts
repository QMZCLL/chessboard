// 复习卡片的分组、计数与文字：列表不透露答案，复习后的文字说明这次与原来的思考。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { ReviewCard, ReviewItem, ReviewStatus } from "../../src/api/types.ts";
import {
  attemptText,
  counts,
  filterItems,
  historyText,
  nextTodoId,
  positionLabel,
  reviewCardId,
  suggestedStatus,
  thinkingText,
  whenText,
} from "../../src/state/reviewList.ts";

function item(id: string, status: ReviewStatus, history: string[] = [], complete = true): ReviewItem {
  const card = {
    id,
    status,
    turn: 3,
    side: "black",
    history: history.map((notation) => ({ mv: "a0a1", notation })),
    history_complete: complete,
    answer: { mv: "h9g7", notation: "马8进7" },
  } as unknown as ReviewCard;
  return { card, source_available: true, study_name: "测试" };
}

test("待复习包括待复习与需要再练，已理解单独一组", () => {
  const items = [item("a", "again"), item("b", "due"), item("c", "understood")];
  assert.deepEqual(filterItems(items, "todo").map((i) => i.card.id), ["a", "b"]);
  assert.deepEqual(filterItems(items, "understood").map((i) => i.card.id), ["c"]);
  assert.deepEqual(counts(items), { todo: 2, understood: 1, all: 3 });
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

test("下一张待复习的卡片：跳过已理解的，到末尾从头找", () => {
  const items = [item("a", "due"), item("b", "understood"), item("c", "again")];
  assert.equal(nextTodoId(items, "a"), "c");
  assert.equal(nextTodoId(items, "c"), "a");
  assert.equal(nextTodoId([item("a", "due")], "a"), null);
});

test("复习时间与卡片编号", () => {
  const now = new Date(2026, 9, 25, 20, 0).getTime();
  assert.equal(whenText(null, now), "还没有复习");
  assert.equal(whenText(new Date(2026, 9, 25, 9, 5).getTime(), now), "今天 09:05 复习过");
  assert.equal(whenText(new Date(2026, 9, 20, 9, 5).getTime(), now), "10 月 20 日复习过");
  assert.equal(reviewCardId("s1", 12), "s1/0000000012");
});
