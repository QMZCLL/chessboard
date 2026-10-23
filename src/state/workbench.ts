// 学习工作台的统一状态。棋盘、棋谱、讲解面板都从这里取数据，避免各组件直接改动 DOM。

import { computed, reactive } from "vue";
import { api, errorMessage, isTauri, newRequestId, on, pickOpenPath, pickSavePath, ready } from "../api/backend";
import type * as T from "../api/types";
import { applyLine, placementOf } from "../utils/board";

export interface Preview {
  label: string;
  baseNode: number;
  moves: T.Move[];
  notation: string[];
  /** 已经走到第几步（0 表示起点） */
  index: number;
}

export interface Highlight {
  squares: T.Square[];
  arrows: T.Move[];
  evidenceId: string | null;
}

export interface AnalysisState {
  requestId: string;
  studyId: string;
  nodeId: number;
  budget: T.Budget;
  running: boolean;
  depth: number;
  candidates: T.CandidateView[];
  position: T.PositionExplanation | null;
  error: string | null;
}

type Notify = (type: "info" | "success" | "warning" | "error", text: string) => void;

let notify: Notify = (type, text) => console.log(type, text);
let detailSeq = 0;

export const wb = reactive({
  booted: false,
  startupError: null as string | null,
  status: null as T.AppStatus | null,
  view: null as T.StudyView | null,
  selected: 0,
  detail: null as T.NodeDetail | null,
  detailError: null as string | null,
  /** 先想后讲模式下作为试走起点的局面 */
  thinkBase: null as T.NodeDetail | null,
  analysis: null as AnalysisState | null,
  preview: null as Preview | null,
  highlight: null as Highlight | null,
  illegal: null as T.IllegalMove | null,
  mode: "auto" as "auto" | "think",
  revealed: {} as Record<number, boolean>,
  /** 先想后讲：节点 → 用户试走建立的分支节点 */
  attempts: {} as Record<number, number>,
  /** 事件推送来的最新讲解记录（按节点），用于避免较早发出的查询结果覆盖较新的讲解 */
  records: {} as Record<number, T.ExplanationRecord>,
  /** 已收到的最大讲解序号 */
  explainSeq: 0,
  /** AI 讲解状态（按节点），来自节点详情与 coach_updated 事件 */
  coach: {} as Record<number, T.CoachView>,
  capture: { state: "idle" } as T.CaptureState,
  followLive: true,
  dialogs: { newStudy: false, capture: false, settings: false, studies: false },
});

export function setNotifier(fn: Notify) {
  notify = fn;
}

export const study = computed(() => wb.view?.study ?? null);

export function nodeOf(id: number | null | undefined): T.StudyNode | null {
  if (id === null || id === undefined || !wb.view) return null;
  return wb.view.study.nodes[String(id)] ?? null;
}

export const selectedNode = computed(() => nodeOf(wb.selected));

/** 先想后讲：当前节点的讲解尚未揭示，棋盘显示落子前局面让用户先思考。 */
export const thinking = computed(() => {
  const node = selectedNode.value;
  return wb.mode === "think" && !!node?.mv && !wb.revealed[node.id] && !wb.preview;
});

/** 棋盘当前显示的局面 FEN（含行棋方）。 */
export const displayFen = computed(() => {
  const node = selectedNode.value;
  if (!node) return null;
  if (wb.preview) {
    const base = nodeOf(wb.preview.baseNode);
    if (!base) return node.fen;
    const moves = wb.preview.moves.slice(0, wb.preview.index);
    const side = moves.length % 2 === 0 ? base.side_to_move : base.side_to_move === "red" ? "black" : "red";
    return `${applyLine(base.fen, moves)} ${side === "red" ? "w" : "b"}`;
  }
  if (thinking.value) return nodeOf(node.parent)?.fen ?? node.fen;
  return node.fen;
});

export const displayLastMove = computed<T.Move | null>(() => {
  if (wb.preview) return wb.preview.index > 0 ? wb.preview.moves[wb.preview.index - 1] : null;
  const node = selectedNode.value;
  if (!node) return null;
  if (thinking.value) return nodeOf(node.parent)?.mv ?? null;
  return node.mv;
});

export const displayLegal = computed<T.Move[]>(() => {
  if (wb.preview) return [];
  if (thinking.value) return wb.thinkBase?.legal_moves ?? [];
  return wb.detail?.node.id === wb.selected ? wb.detail.legal_moves : [];
});

export const displaySide = computed<T.Color | null>(() => {
  const fen = displayFen.value;
  if (!fen) return null;
  return fen.split(" ")[1] === "b" ? "black" : "red";
});

// ---------- 启动与事件 ----------

export async function boot() {
  await ready();
  on<T.EngineState>("engine_status", (s) => {
    if (wb.status) wb.status.engine = s;
  });
  on<T.ExplanationEvent>("explanation_updated", onExplanation);
  on<T.StudyUpdatedEvent>("study_updated", onStudyUpdated);
  on<T.CoachView>("coach_updated", onCoach);
  on<T.AnalysisProgressEvent>("analysis_progress", (p) => {
    const a = wb.analysis;
    if (!a || a.requestId !== p.request_id) return;
    a.depth = p.depth;
    a.candidates = p.candidates;
  });
  on<T.AnalysisDoneEvent>("analysis_done", (p) => {
    const a = wb.analysis;
    if (!a || a.requestId !== p.request_id) return;
    a.running = false;
    a.position = p.position;
    a.candidates = p.position.candidates;
    a.error = p.error;
    if (p.cancelled) a.error = "分析已取消";
  });
  on<T.CaptureState>("capture_state", (s) => {
    wb.capture = s;
    if (s.state === "awaiting_start" || s.state === "awaiting_confirmation") wb.dialogs.capture = true;
    if (s.state === "stopped") notify("warning", `屏幕连接已停止：${s.message}`);
  });
  on<T.CloudEvent>("cloud_result", (p) => {
    if (p.error) notify("warning", p.error);
    if (p.record && wb.detail && wb.detail.study_id === p.study_id && wb.detail.node.id === p.node_id) {
      wb.detail.cloud = p.record;
    }
  });
  try {
    wb.startupError = await api.startupError();
    if (wb.startupError) {
      wb.booted = true;
      return;
    }
    wb.status = await api.status();
    wb.capture = wb.status.capture;
    for (const n of wb.status.notices) notify("warning", n);
    const last = safeStorage("get", "xq-last-study");
    if (last) {
      try {
        await openStudy(last);
      } catch {
        wb.dialogs.newStudy = true;
      }
    } else {
      wb.dialogs.newStudy = true;
    }
  } catch (e) {
    wb.startupError = errorMessage(e);
  }
  wb.booted = true;
}

function safeStorage(op: "get" | "set", key: string, value?: string): string | null {
  try {
    if (op === "get") return localStorage.getItem(key);
    localStorage.setItem(key, value ?? "");
  } catch {
    // 浏览器存储不可用时只是不记住上次的会话
  }
  return null;
}

export async function refreshStatus() {
  try {
    wb.status = await api.status();
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

/** 两条讲解记录中较新的一条。 */
export function newestRecord(
  a: T.ExplanationRecord | null | undefined,
  b: T.ExplanationRecord | null | undefined,
): T.ExplanationRecord | null {
  if (!a) return b ?? null;
  if (!b) return a;
  return b.updated_at > a.updated_at ? b : a;
}

function onExplanation(p: T.ExplanationEvent) {
  wb.explainSeq = Math.max(wb.explainSeq, p.seq);
  if (!wb.view || wb.view.study.id !== p.study_id) return;
  wb.view.explain_status[String(p.node_id)] = p.status;
  wb.view.stats = p.stats;
  wb.records[p.node_id] = newestRecord(wb.records[p.node_id], p.record)!;
  if (wb.detail && wb.detail.study_id === p.study_id && wb.detail.node.id === p.node_id) {
    wb.detail.explanation = newestRecord(wb.detail.explanation, p.record);
  }
}

/**
 * 两份 AI 讲解状态中较新的一份：先比较记录的更新时间；相同时以后到达的为准
 * （证据更新、设置改变时状态会变而记录不变）。
 */
function newerCoach(a: T.CoachView | undefined, b: T.CoachView): T.CoachView {
  if (!a) return b;
  return (a.record?.updated_at ?? 0) > (b.record?.updated_at ?? 0) ? a : b;
}

function onCoach(p: T.CoachView) {
  if (!wb.view || wb.view.study.id !== p.study_id) return;
  wb.coach[p.node_id] = newerCoach(wb.coach[p.node_id], p);
}

/** 当前节点的 AI 讲解状态。 */
export const coachView = computed(() => (selectedNode.value?.mv ? (wb.coach[wb.selected] ?? null) : null));

/** 生成（或重新生成）当前这一着的 AI 讲解。 */
export async function requestCoach(refresh = false) {
  const s = study.value;
  if (!s || !selectedNode.value?.mv) return;
  const nodeId = wb.selected;
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

/** 设置为“查看时自动生成”时，还没有结果或结果已过期就排队生成；已有有效结果直接使用。 */
function autoCoach(view: T.CoachView | null) {
  const status = wb.status;
  if (!view || !status?.coach.ready || status.config.coach.auto_explain_scope !== "viewed") return;
  if (view.status === "none" || view.status === "stale") void requestCoach(false);
}

async function onStudyUpdated(p: T.StudyUpdatedEvent) {
  if (!wb.view || wb.view.study.id !== p.study_id || p.revision <= wb.view.study.revision) return;
  const previousLive = wb.view.study.live_node;
  try {
    setView(await api.studyView(p.study_id));
  } catch (e) {
    notify("error", errorMessage(e));
    return;
  }
  // 用户停留在实际对局最新节点时才跟随；正在阅读其他节点时不切走
  const live = wb.view.study.live_node;
  if (wb.followLive && live !== null && live !== previousLive && (previousLive === null || wb.selected === previousLive)) {
    await selectNode(live);
  } else if (!nodeOf(wb.selected)) {
    await selectNode(0);
  }
}

// ---------- 学习会话 ----------

function lastMainline(view: T.StudyView): number {
  let id = 0;
  for (;;) {
    const next = view.study.nodes[String(id)]?.children[0];
    if (next === undefined) return id;
    id = next;
  }
}

export async function openStudy(id: string, select?: number) {
  const view = await api.loadStudy(id);
  setView(view);
  safeStorage("set", "xq-last-study", id);
  await selectNode(select ?? view.study.live_node ?? lastMainline(view));
}

/**
 * 采用后台返回的会话视图。命令的响应可能晚于讲解事件到达，此时视图中的讲解状态已过期，
 * 保留棋谱树并重新获取一次视图。
 */
function setView(view: T.StudyView) {
  if (!wb.view || wb.view.study.id !== view.study.id) {
    wb.revealed = {};
    wb.attempts = {};
    wb.records = {};
    wb.coach = {};
    wb.analysis = null;
  }
  wb.view = view;
  if (view.explain_seq < wb.explainSeq) void refreshView(view.study.id);
}

async function refreshView(id: string) {
  try {
    const fresh = await api.studyView(id);
    if (wb.view?.study.id === id && fresh.study.revision >= wb.view.study.revision) wb.view = fresh;
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

export async function createStudy(req: T.CreateStudyRequest): Promise<T.ImportResponse> {
  const resp = await api.createStudy(req);
  setView(resp.study);
  safeStorage("set", "xq-last-study", resp.study.study.id);
  for (const n of resp.notes) notify("warning", n);
  await selectNode(req.kind.type === "record" ? lastMainline(resp.study) : 0);
  return resp;
}

export async function importText(text: string, name: string | null) {
  const resp = await api.importText(text, name);
  setView(resp.study);
  safeStorage("set", "xq-last-study", resp.study.study.id);
  for (const n of resp.notes) notify(resp.stopped ? "warning" : "success", n);
  await selectNode(lastMainline(resp.study));
}

export async function importFromFile() {
  const path = await pickOpenPath();
  if (!path) return;
  const resp = await api.importFile(path);
  setView(resp.study);
  safeStorage("set", "xq-last-study", resp.study.study.id);
  for (const n of resp.notes) notify(resp.stopped ? "warning" : "success", n);
  await selectNode(lastMainline(resp.study));
}

export async function exportStudy() {
  const s = study.value;
  if (!s) return;
  const fileName = `${s.name.replace(/[\\/:*?"<>|]/g, "_")}.json`;
  if (isTauri) {
    const path = await pickSavePath(fileName);
    if (!path) return;
    await api.exportStudyToFile(s.id, path);
    notify("success", `已导出到 ${path}`);
  } else {
    const text = await api.exportStudy(s.id);
    const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = fileName;
    a.click();
    URL.revokeObjectURL(url);
  }
}

export async function updateStudyPrefs(prefs: { name?: string; orientation?: T.Orientation; user_side?: T.Color }) {
  const s = study.value;
  if (!s) return;
  setView(await api.updateStudy({ study_id: s.id, ...prefs }));
}

export async function flipBoard() {
  const s = study.value;
  if (!s) return;
  await updateStudyPrefs({ orientation: s.orientation === "red_bottom" ? "black_bottom" : "red_bottom" });
}

// ---------- 节点与讲解 ----------

export async function selectNode(id: number) {
  const s = study.value;
  if (!s || !nodeOf(id)) return;
  const changed = wb.selected !== id;
  wb.selected = id;
  wb.preview = null;
  wb.highlight = null;
  wb.illegal = null;
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

let thinkSeq = 0;

async function loadThinkBase() {
  const s = study.value;
  const node = selectedNode.value;
  if (!s || !node || node.parent === null) return;
  const seq = ++thinkSeq;
  try {
    const base = await api.nodeDetail(s.id, node.parent);
    // 快速切换节点时，只采用最后一次请求、且仍对应当前节点的结果
    if (seq === thinkSeq && study.value?.id === s.id && selectedNode.value?.parent === base.node.id) wb.thinkBase = base;
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

export function reveal(id = wb.selected) {
  wb.revealed[id] = true;
  if (id === wb.selected) autoCoach(wb.coach[id] ?? null);
}

export async function setMode(mode: "auto" | "think") {
  wb.mode = mode;
  if (thinking.value) await loadThinkBase();
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
    if (thinkingNow) {
      if (target !== node.id) wb.attempts[node.id] = target;
      reveal(node.id);
      notify(target === node.id ? "success" : "info", target === node.id ? "与实际着法相同" : "已建立你的试走分支，下面对照讲解");
      await loadDetail();
      return;
    }
    await selectNode(target);
  } catch (e) {
    notify("error", errorMessage(e));
  }
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

// ---------- 演示与高亮 ----------

export function showEvidence(ev: T.Evidence) {
  wb.highlight = { squares: ev.squares, arrows: ev.arrows, evidenceId: ev.id };
  if (ev.line && ev.line.moves.length) {
    showLine(ev.text.slice(0, 24), ev.line);
  }
}

export function showLine(label: string, line: T.LineRef) {
  const node = selectedNode.value;
  if (!node) return;
  const baseNode = line.base === "parent" ? node.parent : node.id;
  if (baseNode === null) return;
  wb.preview = { label, baseNode, moves: line.moves, notation: line.notation, index: 1 };
}

export function showCandidate(c: T.CandidateView, base: "parent" | "node") {
  showLine(`${c.notation} 的变化`, { base, moves: c.pv, notation: c.pv_notation });
}

export function stepPreview(delta: number) {
  const p = wb.preview;
  if (!p) return;
  p.index = Math.max(0, Math.min(p.moves.length, p.index + delta));
}

export function exitPreview() {
  wb.preview = null;
}

export async function addPreviewAsBranch() {
  const s = study.value;
  const p = wb.preview;
  if (!s || !p) return;
  const moves = p.moves.slice(0, Math.max(1, p.index));
  try {
    const resp = await api.addLine(s.id, p.baseNode, moves);
    if (resp.study) setView(resp.study);
    notify(resp.created ? "success" : "info", resp.created ? "已把这条变化加入棋谱分支" : "这条变化已在棋谱中");
    if (resp.node_id !== null) await selectNode(resp.node_id);
  } catch (e) {
    notify("error", errorMessage(e));
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

export async function saveNote(note: string, starred: boolean) {
  const s = study.value;
  if (!s) return;
  setView(await api.setNote(s.id, wb.selected, note, starred));
}

export function goRelative(delta: number) {
  const node = selectedNode.value;
  if (!node) return;
  if (delta < 0 && node.parent !== null) void selectNode(node.parent);
  if (delta > 0 && node.children.length) void selectNode(node.children[0]);
}

export function placementForEditor(): string {
  const fen = displayFen.value;
  return fen ? placementOf(fen) : "";
}
