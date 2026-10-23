// 候选评分的显示：边界值不能显示成确定的评分或杀棋。
import assert from "node:assert/strict";
import { test } from "node:test";
import { scoreText } from "../../src/utils/board.ts";

test("普通评分标明边界", () => {
  assert.equal(scoreText({ kind: "cp", value: 30 }, "exact"), "+30");
  assert.equal(scoreText({ kind: "cp", value: 30 }, "lower"), "≥ +30");
  assert.equal(scoreText({ kind: "cp", value: -30 }, "upper"), "≤ -30");
  assert.equal(scoreText({ kind: "cp", value: 0 }), "0");
});

test("将杀边界值不显示为几步杀", () => {
  assert.equal(scoreText({ kind: "mate", value: 3 }, "exact"), "3 步杀");
  assert.equal(scoreText({ kind: "mate", value: -2 }, "exact"), "2 步被杀");
  for (const bound of ["lower", "upper"] as const) {
    const text = scoreText({ kind: "mate", value: 3 }, bound);
    assert.equal(text, "将杀边界，待确认");
    assert.ok(!text.includes("步杀"));
  }
});
