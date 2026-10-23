// AI 讲解的显示方式，只由后台给出的状态与“结果是否对应当前条件”决定，界面不另做推断。
// - current：结果对应当前的证据与设置，作为讲解正文；
// - previous：只有上次的结果（已过期、更新失败或正在更新），正文仍用规则讲解，上次结果折叠显示并注明；
// - entry：还没有结果，只显示一行入口。
import type { CoachView } from "../api/types";

export type BadgeTone = "success" | "info" | "warning" | "error";

export interface CoachBadge {
  text: string;
  tone: BadgeTone;
}

export type CoachDisplay =
  | { kind: "hidden" }
  | { kind: "entry" }
  | { kind: "current" | "previous"; badge: CoachBadge; busy: boolean };

export function coachDisplay(view: CoachView | null | undefined): CoachDisplay {
  if (!view) return { kind: "hidden" };
  if (!view.record?.result) return { kind: "entry" };
  const busy = view.status === "queued" || view.status === "running";
  if (view.result_current) {
    if (view.status === "queued") return { kind: "current", badge: { text: "等待生成", tone: "info" }, busy };
    if (view.status === "running") return { kind: "current", badge: { text: "生成中", tone: "info" }, busy };
    // 只有后台明确返回 ready 时才显示“已生成”
    if (view.status === "ready") return { kind: "current", badge: { text: "已生成", tone: "success" }, busy };
    return { kind: "current", badge: { text: "状态未知", tone: "warning" }, busy };
  }
  switch (view.status) {
    case "failed":
      return { kind: "previous", badge: { text: "更新失败", tone: "error" }, busy };
    case "queued":
    case "running":
      return { kind: "previous", badge: { text: "正在更新", tone: "info" }, busy };
    default:
      return { kind: "previous", badge: { text: "已过期", tone: "warning" }, busy };
  }
}
