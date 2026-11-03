// AI 讲解：当前这一着的讲解状态、生成与取消，以及“查看时自动生成”的节奏（停留一会儿才发出，离开时取消没有发出的）。

import { computed, watch } from "vue";
import { api, errorMessage } from "../api/backend";
import type * as T from "../api/types";
import { newerCoach } from "./coachMerge";
import { notify, selectedNode, study, thinking, wb } from "./core";

export function onCoach(p: T.CoachView) {
  if (!wb.view || wb.view.study.id !== p.study_id) return;
  wb.coach[p.node_id] = newerCoach(wb.coach[p.node_id], p);
  // 自动请求已经有了结论，离开时不必再取消（自动请求只对未生成或已过期的着法发起，
  // 所以这里的“已生成”“失败”都来自这次请求）
  if (p.status === "ready" || p.status === "failed") autoPending.delete(`${p.study_id}:${p.node_id}`);
}

/** 当前节点的 AI 讲解状态。 */
export const coachView = computed(() => (selectedNode.value?.mv ? (wb.coach[wb.selected] ?? null) : null));

/**
 * 生成（或重新生成）当前这一着的 AI 讲解。
 * auto：查看时自动发起的请求，离开这一着时如果还没有发出就取消；用户点按发起的请求不取消。
 */
export async function requestCoach(refresh = false, auto = false) {
  const s = study.value;
  if (!s || !selectedNode.value?.mv) return;
  const nodeId = wb.selected;
  if (auto) autoPending.add(`${s.id}:${nodeId}`);
  else autoPending.delete(`${s.id}:${nodeId}`);
  try {
    const view = await api.coachExplain(s.id, nodeId, refresh);
    if (study.value?.id === s.id) wb.coach[nodeId] = newerCoach(wb.coach[nodeId], view);
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

export async function cancelCoach() {
  const s = study.value;
  if (!s) return;
  try {
    await api.coachCancel(s.id, wb.selected);
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

/** 设置或密钥改变后，重新获取当前这一着的 AI 讲解状态。 */
export async function refreshCoach() {
  const s = study.value;
  if (!s || !selectedNode.value?.mv) return;
  const nodeId = wb.selected;
  try {
    const view = await api.coachView(s.id, nodeId);
    if (study.value?.id !== s.id) return;
    wb.coach[nodeId] = newerCoach(wb.coach[nodeId], view);
    if (!thinking.value) autoCoach(wb.coach[nodeId]);
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

/** 自动生成前需要在这一着上停留的时间：快速翻阅经过的着法不发请求。 */
export const AUTO_COACH_DWELL_MS = 500;
let autoTimer: ReturnType<typeof setTimeout> | null = null;
/** 自动发起、尚未完成的请求（会话:节点）：离开这一着时取消其中尚未发出的 */
const autoPending = new Set<string>();

/**
 * 设置为“查看时自动生成”时，还没有结果或结果已过期，且在这一着上停留一会儿后才排队生成；
 * 已有有效结果直接使用。引擎分析还没完成时，后台会等分析完成再生成。
 */
export function autoCoach(view: T.CoachView | null) {
  const status = wb.status;
  if (!view || !status?.coach.ready || status.config.coach.auto_explain_scope !== "viewed") return;
  if (view.status !== "none" && view.status !== "stale") return;
  const { study_id: sid, node_id: nid } = view;
  if (autoTimer) clearTimeout(autoTimer);
  autoTimer = setTimeout(() => {
    autoTimer = null;
    const current = wb.coach[nid];
    if (study.value?.id !== sid || wb.selected !== nid || thinking.value || !current) return;
    if (current.status !== "none" && current.status !== "stale") return;
    void requestCoach(false, true);
  }, AUTO_COACH_DWELL_MS);
}

/** 这一着的请求改由别处（批量生成、手动生成）负责：离开时不再取消。 */
export function keepCoachRequest(studyId: string, nodeId: number) {
  autoPending.delete(`${studyId}:${nodeId}`);
}

// 离开某一着：停止计时；自动发起的请求如果还没有发出就不再发送，已经发出的照常完成并归属这一着
watch(
  () => (study.value ? `${study.value.id}:${wb.selected}` : null),
  (_, previous) => {
    if (autoTimer) {
      clearTimeout(autoTimer);
      autoTimer = null;
    }
    if (previous && autoPending.delete(previous)) {
      const at = previous.lastIndexOf(":");
      void api.coachLeave(previous.slice(0, at), Number(previous.slice(at + 1))).catch(() => undefined);
    }
  },
);
