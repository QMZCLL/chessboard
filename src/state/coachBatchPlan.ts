// 批量生成 AI 讲解的范围与计划：选哪些着法、哪些已有结果可以跳过、大约调用几次模型、今天还剩几次。
// 只依赖数据，不依赖界面状态，便于单独测试。
import type { CoachAvailability, CoachView, Study, StudyNode } from "../api/types";

export type BatchScope = "starred" | "line" | "mainline";

export const SCOPE_LABEL: Record<BatchScope, string> = {
  starred: "收藏的着法",
  line: "从当前着法到这条线的最后",
  mainline: "整条主线",
};

function node(study: Study, id: number | undefined): StudyNode | null {
  return id === undefined ? null : (study.nodes[String(id)] ?? null);
}

/** 沿每一着的第一个后续着法走到这条线的最后（包含起点）。 */
function lineFrom(study: Study, start: number): number[] {
  const ids: number[] = [];
  for (let cur = node(study, start); cur; cur = node(study, cur.children[0])) ids.push(cur.id);
  return ids;
}

/**
 * 范围内的着法，按棋谱顺序排列；起始局面与缺口没有着法，不在其中。
 * - starred：全部收藏的着法（先主线后分支，按棋谱中的显示顺序）；
 * - line：从选中的着法开始沿这条线到最后，选中起始局面时从第一着开始；
 * - mainline：主线上的全部着法。
 */
export function batchTargets(study: Study, scope: BatchScope, selected: number): number[] {
  let ids: number[];
  if (scope === "mainline") ids = lineFrom(study, 0);
  else if (scope === "line") ids = lineFrom(study, selected);
  else {
    ids = [];
    const visit = (id: number) => {
      const n = node(study, id);
      if (!n) return;
      if (n.starred) ids.push(n.id);
      n.children.forEach(visit);
    };
    visit(0);
  }
  return ids.filter((id) => !!node(study, id)?.mv);
}

/** 结果对应当前的证据与设置，不需要再生成。 */
export function hasCurrentResult(view: CoachView | null | undefined): boolean {
  return !!view && view.status === "ready" && view.result_current;
}

export interface BatchPlan {
  /** 需要生成的着法 */
  need: number[];
  /** 已有对应当前条件的结果，直接使用 */
  skip: number[];
  /**
   * 预计调用模型的次数（按后台的重试策略）：usual 每着一次；repair 输出需要修正时；
   * max 请求还遇到网络错误、限流或服务端错误而重试时的最多次数
   */
  calls: { usual: number; repair: number; max: number };
  /** 今天还能调用几次；null 表示不限 */
  left: number | null;
  /** 今天剩余的次数可能不够生成全部 */
  short: boolean;
}

export function budgetLeft(coach: Pick<CoachAvailability, "used_today" | "daily_budget">): number | null {
  return coach.daily_budget === 0 ? null : Math.max(0, coach.daily_budget - coach.used_today);
}

export function planBatch(
  targets: number[],
  views: Record<number, CoachView | null | undefined>,
  coach: Pick<CoachAvailability, "used_today" | "daily_budget" | "calls_per_move">,
): BatchPlan {
  const need = targets.filter((id) => !hasCurrentResult(views[id]));
  const skip = targets.filter((id) => hasCurrentResult(views[id]));
  const left = budgetLeft(coach);
  const n = need.length;
  const policy = coach.calls_per_move;
  const calls = { usual: n * policy.usual, repair: n * policy.with_repair, max: n * policy.max };
  return { need, skip, calls, left, short: left !== null && left < calls.usual };
}

/** 生成失败是因为今天的调用次数已经用完：之后的着法也不会成功，应停止。 */
export function budgetExhausted(reason: string | null | undefined): boolean {
  return !!reason && reason.includes("调用次数已达上限");
}

export type Outcome = { kind: "pending" } | { kind: "done" } | { kind: "failed"; reason: string };

/**
 * 发出请求之后，根据最新的状态判断这一着是否已有结论。
 * busy 表示发出请求之后已经看到过排队或生成中；startedAt 是发出请求的时间。
 * 在看到排队之前，较早的“失败”或“未生成”可能还是这次请求之前的状态，不能当作这次的结果。
 */
export function outcomeOf(view: CoachView | null | undefined, busy: boolean, startedAt: number): Outcome {
  if (!view) return { kind: "pending" };
  if (hasCurrentResult(view)) return { kind: "done" };
  if (view.status === "disabled") return { kind: "failed", reason: view.reason ?? "AI 讲解当前不可用" };
  if (view.status === "queued" || view.status === "running") return { kind: "pending" };
  const fresh = busy || (view.record?.updated_at ?? 0) >= startedAt;
  if (!fresh) return { kind: "pending" };
  if (view.status === "failed") return { kind: "failed", reason: view.reason ?? "生成失败" };
  // 排队后又回到未生成或已过期：请求被取消，或依据在生成期间改变
  if (busy) return { kind: "failed", reason: view.reason ?? "请求已取消，没有生成" };
  return { kind: "pending" };
}
