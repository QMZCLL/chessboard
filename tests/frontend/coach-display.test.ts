// AI 讲解的显示方式：只有对应当前条件的结果才作为正文；更新失败后保留的旧结果不能显示为“已生成”。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { CoachResult, CoachView } from "../../src/api/types.ts";
import { coachDisplay } from "../../src/state/coachDisplay.ts";

function view(status: CoachView["status"], withResult: boolean, current: boolean, reason: string | null = null): CoachView {
  return {
    seq: 1,
    study_id: "s",
    node_id: 1,
    status,
    reason,
    result_current: current,
    record: {
      study_id: "s",
      node_id: 1,
      state: status === "failed" ? "failed" : "ready",
      waiting_evidence: false,
      generation: 2,
      result: withResult ? ({} as CoachResult) : null,
      error: status === "failed" ? "网络错误" : null,
      updated_at: 1,
    },
  };
}

test("更新失败且只保留旧结果时，显示为“更新失败”的上次讲解，不显示成功", () => {
  const d = coachDisplay(view("failed", true, false, "网络错误"));
  assert.equal(d.kind, "previous");
  assert.ok(d.kind === "previous" && d.badge.text === "更新失败" && d.badge.tone === "error");
});

test("结果仍然有效、只是重新生成失败时，仍作为正文并显示“已生成”", () => {
  const d = coachDisplay(view("ready", true, true, "最近一次重新生成失败：网络错误"));
  assert.equal(d.kind, "current");
  assert.ok(d.kind === "current" && d.badge.tone === "success");
});

test("已过期的结果不作为正文", () => {
  const d = coachDisplay(view("stale", true, false));
  assert.ok(d.kind === "previous" && d.badge.text === "已过期");
});

test("从过期结果开始更新时，生成期间仍注明是上次的讲解", () => {
  const d = coachDisplay(view("running", true, false));
  assert.ok(d.kind === "previous" && d.busy && d.badge.text === "正在更新");
});

test("重新生成仍然有效的结果时，生成期间继续作为正文", () => {
  const d = coachDisplay(view("running", true, true));
  assert.ok(d.kind === "current" && d.busy && d.badge.tone === "info");
});

test("“已生成”只用于后台返回的 ready", () => {
  for (const status of ["queued", "running", "stale", "failed", "none", "disabled"] as const) {
    for (const current of [true, false]) {
      const d = coachDisplay(view(status, true, current));
      assert.ok(d.kind === "hidden" || d.kind === "entry" || d.badge.tone !== "success", `${status}/${current}`);
    }
  }
});

test("没有结果时只显示入口", () => {
  assert.equal(coachDisplay(view("failed", false, false)).kind, "entry");
  assert.equal(coachDisplay(null).kind, "hidden");
});
