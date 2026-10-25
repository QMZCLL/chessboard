// 批量生成 AI 讲解：先按范围列出需要生成的着法与预计调用次数，确认后逐着生成。
// 一次只发起一着，等它有结论再发起下一着；可以暂停（当前这一着完成后暂停）、继续、停止，
// 失败的着法可以重试；今天的调用次数用完时自动停止。
// 停止时只取消尚未发出的请求，已经发出的请求完成后，结果仍归入对应的着法。
import { reactive, watch } from "vue";
import { api, errorMessage } from "../api/backend";
import type * as T from "../api/types";
import { newerCoach } from "./coachMerge";
import { batchTargets, budgetExhausted, budgetLeft, outcomeOf, planBatch, type BatchPlan, type BatchScope, type Outcome } from "./coachBatchPlan";
import { keepCoachRequest, refreshStatus, study, wb } from "./workbench";

export type ItemState = "waiting" | "running" | "done" | "failed";

export interface BatchItem {
  id: number;
  state: ItemState;
  reason: string | null;
}

export type Phase = "idle" | "running" | "pausing" | "paused" | "stopping" | "finished";

export const batch = reactive({
  scope: "mainline" as BatchScope,
  studyId: null as string | null,
  preparing: false,
  plan: null as BatchPlan | null,
  items: [] as BatchItem[],
  phase: "idle" as Phase,
  /** 结束的原因：用完次数、停止、切换会话等；正常完成时为空 */
  ended: null as string | null,
  error: null as string | null,
});

/** 每着最长等待时间（含等待引擎分析）：超过后记为失败，可以重试 */
const ITEM_TIMEOUT_MS = 10 * 60 * 1000;
const POLL_MS = 1500;

export const batchActive = () => batch.phase === "running" || batch.phase === "pausing" || batch.phase === "stopping";

/** 按范围列出着法并读取它们当前的 AI 讲解状态，得出需要生成的着法。运行中不重新计划。 */
export async function prepareBatch(scope: BatchScope = batch.scope) {
  const s = study.value;
  if (!s || batchActive() || batch.phase === "paused") return;
  batch.scope = scope;
  batch.studyId = s.id;
  batch.preparing = true;
  batch.error = null;
  batch.ended = null;
  batch.phase = "idle";
  try {
    const targets = batchTargets(s, scope, wb.selected);
    const views: Record<number, T.CoachView> = {};
    await Promise.all(
      targets.map(async (id) => {
        const v = await api.coachView(s.id, id);
        views[id] = v;
        if (study.value?.id === s.id) wb.coach[id] = newerCoach(wb.coach[id], v);
      }),
    );
    await refreshStatus();
    if (study.value?.id !== s.id || batch.scope !== scope || !wb.status) return;
    batch.plan = planBatch(targets, views, wb.status.coach);
    batch.items = batch.plan.need.map((id) => ({ id, state: "waiting", reason: null }));
  } catch (e) {
    batch.error = errorMessage(e);
    batch.plan = null;
    batch.items = [];
  } finally {
    batch.preparing = false;
  }
}

export function startBatch() {
  if (batchActive() || !batch.items.some((i) => i.state === "waiting")) return;
  batch.ended = null;
  batch.phase = "running";
  void run();
}

/** 当前这一着完成后暂停 */
export function pauseBatch() {
  if (batch.phase === "running") batch.phase = "pausing";
}

export function resumeBatch() {
  if (batch.phase === "pausing") batch.phase = "running";
  else if (batch.phase === "paused") startBatch();
}

/** 停止：当前这一着如果还没有发出就不再发送，之后的着法都不再生成 */
export function stopBatch() {
  if (batchActive()) batch.phase = "stopping";
  else if (batch.phase === "paused") finish("已停止");
}

/** 把失败的着法重新列为待生成并开始 */
export function retryFailed() {
  if (batchActive()) return;
  for (const item of batch.items) {
    if (item.state === "failed") {
      item.state = "waiting";
      item.reason = null;
    }
  }
  startBatch();
}

function finish(reason: string | null) {
  batch.phase = "finished";
  batch.ended = reason;
}

async function run() {
  const sid = batch.studyId;
  for (;;) {
    if (batch.phase === "pausing") return void (batch.phase = "paused");
    if (batch.phase === "stopping") return finish("已停止");
    if (!sid || study.value?.id !== sid) return finish("已切换到其他学习会话，批量生成已停止");
    const item = batch.items.find((i) => i.state === "waiting");
    if (!item) return finish(null);
    if (wb.status && budgetLeft(wb.status.coach) === 0) return finish("今天的调用次数已用完，已停止");
    item.state = "running";
    const outcome = await generate(sid, item.id);
    if (outcome.kind === "pending") {
      // 停止时这一着还没有结论：已经发出的请求在后台完成，结果归入这一着
      item.state = "waiting";
      return finish("已停止");
    }
    item.state = outcome.kind === "done" ? "done" : "failed";
    item.reason = outcome.kind === "failed" ? outcome.reason : null;
    await refreshStatus();
    if (outcome.kind === "failed" && budgetExhausted(outcome.reason)) return finish("今天的调用次数已用完，已停止");
    if (outcome.kind === "failed" && wb.status && !wb.status.coach.ready) {
      return finish(wb.status.coach.reason ?? "AI 讲解当前不可用，已停止");
    }
  }
}

/** 发起一着的生成并等待结论；停止时返回 pending。 */
async function generate(sid: string, nid: number): Promise<Outcome> {
  const startedAt = Date.now();
  keepCoachRequest(sid, nid);
  let latest: T.CoachView | undefined;
  try {
    latest = await api.coachExplain(sid, nid, false);
  } catch (e) {
    return { kind: "failed", reason: errorMessage(e) };
  }
  let busy = false;
  let left = false;
  const deadline = Date.now() + ITEM_TIMEOUT_MS;
  for (;;) {
    if (study.value?.id === sid && wb.coach[nid]) latest = newerCoach(latest, wb.coach[nid]);
    if (latest.status === "queued" || latest.status === "running") busy = true;
    const outcome = outcomeOf(latest, busy, startedAt);
    if (outcome.kind !== "pending") return outcome;
    if (batch.phase === "stopping") {
      if (!left) {
        left = true;
        await api.coachLeave(sid, nid).catch(() => undefined);
      }
      return { kind: "pending" };
    }
    if (Date.now() > deadline) return { kind: "failed", reason: "等待时间过长，可以稍后重试" };
    if (!(await changed(sid, nid))) {
      try {
        latest = newerCoach(latest, await api.coachView(sid, nid));
      } catch (e) {
        return { kind: "failed", reason: errorMessage(e) };
      }
    }
  }
}

/** 等待这一着的状态推送或批量操作改变（停止、暂停）；一段时间没有变化时返回 false，由调用方查询一次。 */
function changed(sid: string, nid: number): Promise<boolean> {
  return new Promise((resolve) => {
    const stop = watch(
      () => [study.value?.id === sid ? wb.coach[nid] : null, batch.phase] as const,
      () => done(true),
    );
    const timer = setTimeout(() => done(false), POLL_MS);
    function done(v: boolean) {
      stop();
      clearTimeout(timer);
      resolve(v);
    }
  });
}
