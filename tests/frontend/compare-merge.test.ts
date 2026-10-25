// 试走对比结果按请求序号合并：较早请求的完整结果不会挡住较新的请求直接返回的结果。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { CompareView } from "../../src/api/types.ts";
import { newerCompare } from "../../src/state/compareMerge.ts";

function view(seq: number, pending: boolean, outcome = "close", reference = "h2e2"): CompareView {
  return {
    seq,
    conditions_key: `cond-${seq}`,
    study_id: "s",
    node_id: 0,
    mine: "c3c4",
    reference,
    pending,
    comparison: { outcome },
  } as unknown as CompareView;
}

test("同一组走法重新比较：新请求直接返回的完整结果代替较早请求的完整结果", () => {
  const old = view(1, false, "reference_better");
  const fresh = view(2, false, "mine_better");
  assert.equal(newerCompare(old, fresh), fresh);
});

test("同一次请求的完整结果先推送到达时，随后到达的尚未完成的响应不覆盖它", () => {
  const done = view(3, false);
  assert.equal(newerCompare(done, view(3, true)), done);
  assert.equal(newerCompare(view(3, true), done), done);
});

test("较早请求迟到的推送不覆盖较新的结果", () => {
  const fresh = view(5, true);
  assert.equal(newerCompare(fresh, view(4, false)), fresh);
});

test("换了对照着法时采用新的结果", () => {
  const other = view(1, true, "close", "b0c2");
  assert.equal(newerCompare(view(2, false), other), other);
});
