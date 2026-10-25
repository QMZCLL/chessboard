// 先想后讲的提示快照：之后的分析更新不改变已经看到的提示；练习记录只保存用到的级数。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { ThinkHints } from "../../src/api/types.ts";
import { snapshotHints, usedHints } from "../../src/state/hintSnapshot.ts";

const hints: ThinkHints = {
  side: "red",
  topic: "development",
  observe: "车马还没有出动",
  focus: { text: "右车、右马仍在底线", squares: ["h0"], arrows: [] },
  moves: [{ mv: "h0g2", notation: "马二进三" }],
  moves_note: "按棋盘位置排列",
};

test("快照与原来的提示互不影响", () => {
  const shown = snapshotHints(hints)!;
  const later = structuredClone(hints);
  later.moves = [{ mv: "h2e2", notation: "炮二平五" }];
  hints.observe = "（分析更新后的提示）";
  assert.equal(shown.observe, "车马还没有出动");
  assert.equal(shown.moves[0].notation, "马二进三");
  hints.observe = "车马还没有出动";
  assert.equal(snapshotHints(null), null);
});

test("只保存用到的级数", () => {
  assert.equal(usedHints(hints, 0), null);
  assert.deepEqual(usedHints(hints, 1), { observe: "车马还没有出动", focus: null, moves: [] });
  assert.deepEqual(usedHints(hints, 2)?.focus?.text, "右车、右马仍在底线");
  assert.equal(usedHints(hints, 2)?.moves.length, 0);
  assert.equal(usedHints(hints, 3)?.moves[0].notation, "马二进三");
  assert.equal(usedHints(null, 3), null);
});
