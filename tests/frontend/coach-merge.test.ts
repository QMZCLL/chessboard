// AI 讲解状态按状态版本合并：较早读取的“已生成”不能覆盖随后推送的“已过期”。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { CoachView } from "../../src/api/types.ts";
import { newerCoach } from "../../src/state/coachMerge.ts";

function view(seq: number, status: CoachView["status"], updatedAt: number): CoachView {
  const record = {
    study_id: "s",
    node_id: 1,
    state: "ready" as const,
    waiting_evidence: false,
    generation: 1,
    result: null,
    error: null,
    updated_at: updatedAt,
  };
  return { seq, study_id: "s", node_id: 1, status, reason: null, record, result_current: false };
}

test("先到的新状态不会被后到的旧响应覆盖", () => {
  // 记录本身没有变化（更新时间相同），只是证据更新后变为过期
  const oldReady = view(3, "ready", 1000);
  const stale = view(5, "stale", 1000);
  let current = newerCoach(undefined, stale);
  current = newerCoach(current, oldReady);
  assert.equal(current.status, "stale");
});

test("版本更大的状态总是被采用", () => {
  const queued = view(4, "queued", 1000);
  const ready = view(6, "ready", 2000);
  assert.equal(newerCoach(queued, ready).status, "ready");
  assert.equal(newerCoach(ready, queued).status, "ready");
});

test("版本相同时采用后到达的", () => {
  assert.equal(newerCoach(view(2, "ready", 1), view(2, "stale", 1)).status, "stale");
});
