// 批量生成 AI 讲解：范围内的着法、可以跳过的已有结果、预计调用次数与今天剩余次数、每着的结论判断。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { CoachView, Study, StudyNode } from "../../src/api/types.ts";
import { batchTargets, budgetExhausted, outcomeOf, planBatch } from "../../src/state/coachBatchPlan.ts";

function n(id: number, parent: number | null, children: number[], starred = false): StudyNode {
  return {
    id,
    parent,
    children,
    mv: parent === null ? null : `m${id}`,
    notation: parent === null ? null : `着${id}`,
    fen: "",
    side_to_move: "red",
    ply: 0,
    source: "record",
    gap: false,
    gap_reason: null,
    note: "",
    starred,
    practice: null,
    created_at: 0,
  };
}

// 主线 0→1→2→3→4；2 有分支 5→6；收藏 3 与 6
const study = {
  nodes: Object.fromEntries(
    [n(0, null, [1]), n(1, 0, [2]), n(2, 1, [3, 5]), n(3, 2, [4], true), n(4, 3, []), n(5, 2, [6]), n(6, 5, [], true)].map((x) => [String(x.id), x]),
  ),
} as unknown as Study;

const POLICY = { usual: 1, with_repair: 2, max: 4 };

function view(status: CoachView["status"], current = false, updatedAt = 0): CoachView {
  const record = { study_id: "s", node_id: 1, state: "idle" as const, waiting_evidence: false, generation: 1, result: null, error: null, updated_at: updatedAt };
  return { seq: 1, study_id: "s", node_id: 1, status, reason: status === "failed" ? "模型输出不合格" : null, record, result_current: current };
}

test("范围：整条主线、这条线到最后、收藏的着法；不含起始局面", () => {
  assert.deepEqual(batchTargets(study, "mainline", 5), [1, 2, 3, 4]);
  assert.deepEqual(batchTargets(study, "line", 5), [5, 6]);
  assert.deepEqual(batchTargets(study, "line", 0), [1, 2, 3, 4]);
  assert.deepEqual(batchTargets(study, "starred", 0), [3, 6]);
});

test("已有对应当前条件的结果直接使用，其余都要生成", () => {
  const views = { 1: view("ready", true), 2: view("stale"), 3: view("failed"), 4: undefined };
  const plan = planBatch([1, 2, 3, 4], views, { used_today: 0, daily_budget: 0, calls_per_move: POLICY });
  assert.deepEqual(plan.skip, [1]);
  assert.deepEqual(plan.need, [2, 3, 4]);
  // 与后台的重试策略一致：修正输出再加一次，每次请求遇到网络错误还会重试一次
  assert.deepEqual(plan.calls, { usual: 3, repair: 6, max: 12 });
  assert.equal(plan.left, null, "上限为 0 表示不限");
  assert.equal(plan.short, false);
});

test("今天剩余的次数不够时提示", () => {
  const plan = planBatch([1, 2, 3], {}, { used_today: 8, daily_budget: 10, calls_per_move: POLICY });
  assert.equal(plan.left, 2);
  assert.equal(plan.short, true);
  assert.equal(planBatch([1], {}, { used_today: 12, daily_budget: 10, calls_per_move: POLICY }).left, 0, "已用超过上限时剩余为 0");
});

test("用完次数的失败会停止批量生成", () => {
  assert.ok(budgetExhausted("今天的 AI 讲解调用次数已达上限（3 次），可在设置中调整"));
  assert.ok(!budgetExhausted("模型输出不合格"));
  assert.ok(!budgetExhausted(null));
});

test("发出请求之前的失败或未生成状态不算这次的结论", () => {
  const started = 1000;
  // 上次失败的记录（更新时间早于这次请求）：还在等这次请求的结果
  assert.equal(outcomeOf(view("failed", false, 500), false, started).kind, "pending");
  // 这次请求之后写入的失败
  const failed = outcomeOf(view("failed", false, 1500), false, started);
  assert.equal(failed.kind, "failed");
  // 看到排队之后的失败
  assert.equal(outcomeOf(view("failed", false, 500), true, started).kind, "failed");
  // 未生成：排队之前还在等，排队之后回到未生成说明请求被取消
  assert.equal(outcomeOf(view("none"), false, started).kind, "pending");
  assert.equal(outcomeOf(view("none"), true, started).kind, "failed");
  assert.equal(outcomeOf(view("queued"), true, started).kind, "pending");
  assert.equal(outcomeOf(view("ready", true), false, started).kind, "done");
  assert.equal(outcomeOf(view("disabled"), false, started).kind, "failed");
});
