// 节点与讲解：选中着法、读取详情与讲解、走子与试走、局面分析，以及棋谱的编辑与前后移动。

import { computed } from "vue";
import { api, errorMessage, newRequestId } from "../api/backend";
import type * as T from "../api/types";
import { autoCoach } from "./coach";
import { newerCoach } from "./coachMerge";
import { lastMainline, nodeOf, notify, rememberPosition, selectedNode, study, thinking, wb } from "./core";
import { newestRecord, setSelectionRepair, setView } from "./studyView";
import { loadThinkBase, recordPractice, reveal } from "./think";

let detailSeq = 0;

// 采用新的视图后选中的着法已经不在时，改选还在的最近一着
setSelectionRepair((id) => void selectNode(id));

/**
 * 节点是否在试走范围内：试走起点本身，或试走分支（从起点走出的试走着法及其后续）中的着法。
 * 从起点沿原有着法前进、选择棋谱中其他着法，都离开试走。
 */
function inTrial(id: number): boolean {
  const t = wb.trial;
  if (!t) return false;
  if (id === t.origin) return true;
  for (let cur = nodeOf(id); cur; cur = cur.parent === null ? null : nodeOf(cur.parent)) {
    if (t.roots.includes(cur.id)) return true;
    if (cur.id === t.origin) return false;
  }
  return false;
}

export async function selectNode(id: number) {
  const s = study.value;
  if (!s || !nodeOf(id)) return;
  const changed = wb.selected !== id;
  wb.selected = id;
  rememberPosition(s.id, id, nodeOf(id)?.ply ?? 0);
  wb.preview = null;
  wb.highlight = null;
  wb.focus = null;
  wb.illegal = null;
  if (wb.trial && !inTrial(id)) wb.trial = null;
  // 选中的着法在折叠的分支里时展开它
  for (let cur = nodeOf(id); cur && cur.parent !== null; cur = nodeOf(cur.parent)) {
    if (wb.collapsed[cur.id]) wb.collapsed[cur.id] = false;
  }
  if (changed && wb.analysis && wb.analysis.nodeId !== id) wb.analysis = null;
  void api.focusNode(s.id, id).catch(() => undefined);
  await loadDetail();
  if (thinking.value) await loadThinkBase();
}

export async function loadDetail() {
  const s = study.value;
  if (!s) return;
  const seq = ++detailSeq;
  const nodeId = wb.selected;
  try {
    const detail = await api.nodeDetail(s.id, nodeId);
    if (seq !== detailSeq) return;
    detail.explanation = newestRecord(detail.explanation, wb.records[nodeId]);
    wb.detail = detail;
    wb.detailError = null;
    if (detail.coach) {
      wb.coach[nodeId] = newerCoach(wb.coach[nodeId], detail.coach);
      if (!thinking.value) autoCoach(wb.coach[nodeId]);
    }
    // 走子节点：由后台判断讲解是否缺失、不完整或因设置改变而过期，需要时重新排队
    if (detail.node.mv && !detail.explanation?.pending) {
      const record = await api.explainMove(s.id, nodeId, "quick");
      if (seq === detailSeq && record && wb.detail) {
        wb.detail.explanation = newestRecord(newestRecord(wb.detail.explanation, record), wb.records[nodeId]);
      }
    }
  } catch (e) {
    if (seq === detailSeq) wb.detailError = errorMessage(e);
  }
}

export async function explainDeeper() {
  const s = study.value;
  if (!s || !selectedNode.value?.mv) return;
  const nodeId = wb.selected;
  try {
    const record = await api.explainMove(s.id, nodeId, "deep");
    // 返回时用户可能已切换到其他节点或会话，只更新对应的节点
    const same = wb.detail?.study_id === s.id && wb.detail.node.id === nodeId;
    if (record && same && record.study_id === s.id && record.node_id === nodeId && wb.detail) {
      wb.detail.explanation = newestRecord(wb.detail.explanation, { ...record, pending: true });
    }
    notify("info", "已安排深入分析，完成后自动更新讲解");
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

/** 重试部分完成的讲解：按原来的分析预算重新搜索。 */
export async function retryExplanation() {
  const s = study.value;
  if (!s || !selectedNode.value?.mv) return;
  const nodeId = wb.selected;
  const budget = wb.detail?.explanation?.budget ?? "quick";
  try {
    // 后台开始重新分析时会推送“进行中”的讲解记录，完成后再推送结果
    await api.explainMove(s.id, nodeId, budget, true);
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

/** 在棋盘上走子：记录走法或建立分支；先想后讲模式下作为用户的思考结果。 */
export async function playMove(mv: T.Move) {
  const s = study.value;
  const node = selectedNode.value;
  if (!s || !node) return;
  const thinkingNow = thinking.value;
  const parent = thinkingNow ? node.parent! : node.id;
  const parentNode = nodeOf(parent);
  const variation = thinkingNow || (parentNode?.children.length ?? 0) > 0;
  // 在已有后续着法的局面上走子即为试走：记住起点，便于返回
  const trialOrigin = !thinkingNow && variation && !wb.trial ? parent : null;
  try {
    const resp = await api.applyMove(s.id, parent, mv, variation);
    if (resp.illegal) {
      wb.illegal = resp.illegal;
      wb.highlight = { squares: resp.illegal.squares, arrows: [], evidenceId: null };
      return;
    }
    wb.illegal = null;
    if (resp.study) setView(resp.study);
    const target = resp.node_id!;
    // 用户自己走出的着法是已知的，先想后讲中不再隐藏或要求思考
    if (target !== node.id) wb.revealed[target] = true;
    if (thinkingNow) {
      void recordPractice(node.id, mv);
      if (target !== node.id) wb.attempts[node.id] = target;
      reveal(node.id);
      notify(target === node.id ? "success" : "info", target === node.id ? "与实际着法相同" : "已建立你的试走分支，下面对照讲解");
      await loadDetail();
      return;
    }
    if (trialOrigin !== null) wb.trial = { origin: trialOrigin, roots: [] };
    // 从试走起点走出的着法是一条试走分支的开头
    if (wb.trial && parent === wb.trial.origin && !wb.trial.roots.includes(target)) wb.trial.roots.push(target);
    await selectNode(target);
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

/** 从当前局面开始试走：之后的走法保存在分支中，可以随时返回。 */
export function startTrial() {
  wb.preview = null;
  wb.trial = { origin: wb.selected, roots: [] };
}

/** 结束试走，回到开始试走的局面；已经建立的分支保留在棋谱中。 */
export async function endTrial() {
  const origin = wb.trial?.origin;
  wb.trial = null;
  if (origin !== undefined) await selectNode(origin);
}

export async function analyze(budget: T.Budget) {
  const s = study.value;
  if (!s) return;
  if (wb.analysis?.running) await cancelAnalysis();
  const requestId = newRequestId();
  wb.analysis = {
    requestId,
    studyId: s.id,
    nodeId: wb.selected,
    budget,
    running: true,
    depth: 0,
    candidates: [],
    position: null,
    error: null,
  };
  try {
    await api.analyzePosition(s.id, wb.selected, budget, requestId);
  } catch (e) {
    if (wb.analysis?.requestId === requestId) {
      wb.analysis.running = false;
      wb.analysis.error = errorMessage(e);
    }
  }
}

export async function cancelAnalysis() {
  const a = wb.analysis;
  if (!a?.running) return;
  await api.cancelAnalysis(a.requestId).catch(() => undefined);
}

export async function cloudLookup() {
  const s = study.value;
  if (!s) return;
  try {
    await api.cloudLookup(s.id, wb.selected);
  } catch (e) {
    notify("warning", errorMessage(e));
  }
}

// ---------- 棋谱编辑 ----------

export async function deleteFrom(id: number) {
  const s = study.value;
  const node = nodeOf(id);
  if (!s || !node || node.parent === null) return;
  setView(await api.deleteNode(s.id, id));
  if (!nodeOf(wb.selected)) await selectNode(node.parent);
}

export async function promote(id: number) {
  const s = study.value;
  if (!s) return;
  setView(await api.promoteNode(s.id, id));
}

/**
 * 保存笔记。会话与着法由调用方给出（见 state/notes.ts）。保存期间切换了会话，或界面已有更新的视图时，
 * 不用这次的响应覆盖当前视图。
 */
export async function saveNote(studyId: string, nodeId: number, note: string, starred: boolean) {
  setView(await api.setNote(studyId, nodeId, note, starred));
}

/** 主线（从起始局面沿第一个子节点）上的节点。 */
export const mainline = computed(() => {
  const ids = new Set<number>();
  for (let cur = nodeOf(0); cur; cur = nodeOf(cur.children[0])) ids.add(cur.id);
  return ids;
});

/** 某着及其之后（含分支）的着法数。 */
export function subtreeSize(id: number): number {
  const node = nodeOf(id);
  if (!node) return 0;
  return 1 + node.children.reduce((n, c) => n + subtreeSize(c), 0);
}

/** 返回主线：回到当前分支从主线分出的那一着。 */
export function backToMainline() {
  let cur = selectedNode.value;
  while (cur && !mainline.value.has(cur.id)) cur = nodeOf(cur.parent);
  if (cur) void selectNode(cur.id);
}

/** 最新局面：屏幕连接时为实际对局的最新着，否则为主线最后一着。 */
export const latestNode = computed(() => {
  const view = wb.view;
  if (!view) return null;
  return view.study.live_node ?? lastMainline(view);
});

export function goRelative(delta: number) {
  const node = selectedNode.value;
  if (!node) return;
  if (delta < 0 && node.parent !== null) void selectNode(node.parent);
  if (delta > 0 && node.children.length) void selectNode(node.children[0]);
}
