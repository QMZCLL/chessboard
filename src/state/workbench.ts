// 学习工作台的统一状态。棋盘、棋谱、讲解面板都从这里取数据，避免各组件直接改动 DOM。

import { computed, reactive } from "vue";
import { api, errorMessage, isTauri, newRequestId, on, pickOpenPath, pickSavePath, ready } from "../api/backend";
import type * as T from "../api/types";
import { applyLine, moveNumber, placementOf, sideLabel } from "../utils/board";
import { newerCoach } from "./coachMerge";

export interface Preview {
  /** 演示的对象，写明是哪一着的什么变化，如“炮二平五的主要应对” */
  title: string;
  /** 发起演示的条目（如某个候选），用于标出正在演示的是哪一行 */
  source: string | null;
  baseNode: number;
  moves: T.Move[];
  notation: string[];
  /** 已经走到第几步（0 表示起点） */
  index: number;
  /** 依据在起点局面上的标记，只在起点（第 0 步）显示 */
  marks: { squares: T.Square[]; arrows: T.Move[] } | null;
}

/** 讲解中被选中的一段及其当前查看的依据。 */
export interface ClaimFocus {
  key: string;
  evidenceId: string | null;
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
  focus: null as ClaimFocus | null,
  /**
   * 试走：从棋谱中某个局面（origin）开始走出自己的着法，走法保存在分支中。
   * roots 是试走中从起点走出的各条分支的第一着；在这些分支里前后移动、回到起点都仍在试走中。
   */
  trial: null as { origin: number; roots: number[] } | null,
  illegal: null as T.IllegalMove | null,
  mode: "auto" as "auto" | "think",
  revealed: {} as Record<number, boolean>,
  /** 先想后讲：节点 → 用户试走建立的分支节点 */
  attempts: {} as Record<number, number>,
  /** 棋谱中折叠起来的分支（按分支第一着的节点） */
  collapsed: {} as Record<number, boolean>,
  /** 事件推送来的最新讲解记录（按节点），用于避免较早发出的查询结果覆盖较新的讲解 */
  records: {} as Record<number, T.ExplanationRecord>,
  /** 已收到的最大讲解序号 */
  explainSeq: 0,
  /** AI 讲解状态（按节点），来自节点详情与 coach_updated 事件 */
  coach: {} as Record<number, T.CoachView>,
  capture: { state: "idle" } as T.CaptureState,
  followLive: true,
  dialogs: { newStudy: false, capture: false, settings: false, studies: false },
  /** 下次打开设置时显示的分组（从其他位置跳转到某项设置时使用） */
  settingsTab: null as SettingsTab | null,
});

export type SettingsTab = "learn" | "coach" | "engine" | "capture" | "diag";

export function openSettings(tab: SettingsTab | null = null) {
  wb.settingsTab = tab;
  wb.dialogs.settings = true;
}

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

/** 某个节点之后下一着的回合数。 */
function roundAfter(node: T.StudyNode): number {
  const rootSide = nodeOf(0)?.side_to_move ?? "red";
  return parseInt(moveNumber(node.ply + 1, rootSide, node.side_to_move), 10);
}

export type BoardContextKind = "record" | "start" | "think" | "trial" | "preview" | "before";

/** 选中的局面已经结束（将死或困毙），不能再走子。 */
export const selectedEnded = computed(() => {
  const d = wb.detail;
  return !!d && d.node.id === wb.selected && (d.status === "checkmate" || d.status === "stalemate");
});

/** 棋盘上方的状态说明：现在看的是哪一个局面，可以做什么。 */
export const boardContext = computed<{ kind: BoardContextKind; text: string; hint: string | null } | null>(() => {
  const node = selectedNode.value;
  const side = displaySide.value;
  if (!node || !side) return null;
  const p = wb.preview;
  if (p && p.moves.length === 0) return { kind: "before", text: p.title, hint: null };
  if (p) return { kind: "preview", text: `正在演示：${p.title} · 第 ${p.index} / ${p.moves.length} 着`, hint: null };
  if (thinking.value) return { kind: "think", text: `先想后讲 · 请为${sideLabel(side)}选择下一步`, hint: null };
  if (wb.trial) {
    const origin = nodeOf(wb.trial.origin);
    const round = origin ? roundAfter(origin) : 1;
    return { kind: "trial", text: `试走分支 · 从第 ${round} 回合开始`, hint: "走法会保存在棋谱的分支中" };
  }
  // 只有还能继续走子时才提示走子
  const end = node.children.length === 0 && !selectedEnded.value;
  if (node.id === 0) {
    return { kind: "start", text: "起始局面", hint: end ? "在棋盘上走子，开始记录" : null };
  }
  return { kind: "record", text: `棋谱局面 · 第 ${roundAfter(node)} 回合`, hint: end ? "在棋盘上走子，继续记录" : null };
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
    // 节点编号只在同一个会话内有意义
    wb.trial = null;
    wb.collapsed = {};
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
    if (thinkingNow) {
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

// ---------- 依据、演示与高亮 ----------

/** 棋盘当前显示的是不是节点本身的局面（而不是落子前的思考局面或演示中的局面）。 */
const boardShowsNode = computed(() => !wb.preview && !thinking.value);

/** 依据能否直接画在当前棋盘上。 */
function evidenceOnBoard(ev: T.Evidence): boolean {
  return boardShowsNode.value && ev.on === "node";
}

function shortLabel(text: string) {
  return text.length > 18 ? `${text.slice(0, 18)}…` : text;
}

/**
 * 选中讲解中的一段：只突出显示依据，不进入演示。依据在当前局面上时高亮棋子与关系；
 * 在落子前局面上时不画在落子后的棋盘上，由界面提示从起点查看。
 */
export function focusEvidence(key: string, ev: T.Evidence | null) {
  wb.preview = null;
  wb.focus = { key, evidenceId: ev?.id ?? null };
  wb.highlight = ev && evidenceOnBoard(ev) && (ev.squares.length || ev.arrows.length)
    ? { squares: ev.squares, arrows: ev.arrows, evidenceId: ev.id }
    : null;
}

/** 讲解的对象：走子节点为这一着，起始局面为“起始局面”。 */
const subject = computed(() => {
  const node = selectedNode.value;
  if (!node) return "";
  return node.mv ? (node.notation ?? "这一着") : "起始局面";
});

/** 退出演示时回到的局面，写在返回按钮上。 */
export const previewBackLabel = computed(() => (selectedNode.value?.mv ? `返回${subject.value}后的局面` : "返回起始局面"));

interface PreviewStart {
  title: string;
  source?: string;
  base: "parent" | "node";
  moves: T.Move[];
  notation: string[];
  marks: Preview["marks"];
}

function startPreview(p: PreviewStart) {
  const node = selectedNode.value;
  if (!node) return;
  const baseNode = p.base === "parent" ? node.parent : node.id;
  if (baseNode === null) return;
  wb.highlight = null;
  // 从起点开始：先看清起始局面与依据标记，再逐着前进
  wb.preview = { title: p.title, source: p.source ?? null, baseNode, moves: p.moves, notation: p.notation, index: 0, marks: p.marks };
}

/**
 * 演示依据中的变化：从依据指定的起始局面开始。变化以本步着法开头、依据又标在本步之后的局面上时
 * （如对方的应对），直接从当前局面开始，不先重放刚走的这一着。
 */
function demoLine(ev: T.Evidence): T.LineRef | null {
  if (!ev.line) return null;
  const { base, moves, notation } = ev.line;
  const line = base === "parent" && ev.on === "node" && moves[0] === selectedNode.value?.mv
    ? { base: "node" as const, moves: moves.slice(1), notation: notation.slice(1) }
    : ev.line;
  return line.moves.length ? line : null;
}

/** 依据是否有可以演示的变化。 */
export function canDemo(ev: T.Evidence | null | undefined): boolean {
  return !!ev && !!demoLine(ev);
}

/** 演示的对象：对方应对写成“X的主要应对”，落子前的另一种走法写成“X之前的候选 Y”。 */
function evidenceTitle(ev: T.Evidence, line: T.LineRef): string {
  const name = subject.value;
  if (ev.topic === "reply") return `${name}的主要应对`;
  if (line.base === "node") return selectedNode.value?.mv ? `${name}之后的变化` : `${name}的变化`;
  return line.notation[0] === name ? `${name}及其后续` : `${name}之前的候选 ${line.notation[0]}`;
}

export function demoEvidence(ev: T.Evidence) {
  const line = demoLine(ev);
  if (!line) return;
  const marks = ev.on === line.base ? { squares: ev.squares, arrows: ev.arrows } : null;
  startPreview({ title: evidenceTitle(ev, line), base: line.base, moves: line.moves, notation: line.notation, marks });
}

/** 在依据所在的局面（落子前）上查看它标出的棋子与关系；所在局面未知的依据不标出。 */
export function viewEvidenceBoard(ev: T.Evidence) {
  if (!ev.on) return;
  const where =
    ev.on === "parent" ? `${subject.value}落子前的局面` : selectedNode.value?.mv ? `${subject.value}后的局面` : subject.value;
  startPreview({ title: `${where} · ${shortLabel(ev.text)}`, base: ev.on, moves: [], notation: [], marks: { squares: ev.squares, arrows: ev.arrows } });
}

/** 候选在演示中的标识。 */
export function candidateSource(c: T.CandidateView, base: "parent" | "node") {
  return `candidate:${base}:${c.mv}`;
}

export function showCandidate(c: T.CandidateView, base: "parent" | "node") {
  if (!c.pv.length) return;
  const name = subject.value;
  const title =
    base === "parent" ? `${name}之前的候选 ${c.notation}` : selectedNode.value?.mv ? `${name}之后的候选 ${c.notation}` : `起始局面的候选 ${c.notation}`;
  startPreview({ title, source: candidateSource(c, base), base, moves: c.pv, notation: c.pv_notation, marks: null });
}

/** 选中候选：在它的起始局面上标出首着；起始局面不是当前棋盘时只显示说明。 */
export function focusCandidate(key: string, c: T.CandidateView, base: "parent" | "node") {
  wb.preview = null;
  wb.focus = { key, evidenceId: null };
  const onBoard = boardShowsNode.value && base === "node";
  wb.highlight = onBoard ? { squares: [c.mv.slice(0, 2), c.mv.slice(2, 4)], arrows: [c.mv], evidenceId: null } : null;
}

export function stepPreview(delta: number) {
  const p = wb.preview;
  if (!p) return;
  p.index = Math.max(0, Math.min(p.moves.length, p.index + delta));
}

export function exitPreview() {
  wb.preview = null;
}

/** 把演示的变化保存为分支：`all` 为整条变化，否则为已经演示到的着法（至少一着）。 */
export async function addPreviewAsBranch(all = false) {
  const s = study.value;
  const p = wb.preview;
  if (!s || !p || !p.moves.length) return;
  const moves = all ? p.moves : p.moves.slice(0, Math.max(1, p.index));
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

/**
 * 保存笔记。会话与着法由调用方给出（见 state/notes.ts）。保存期间切换了会话，或界面已有更新的视图时，
 * 不用这次的响应覆盖当前视图。
 */
export async function saveNote(studyId: string, nodeId: number, note: string, starred: boolean) {
  const view = await api.setNote(studyId, nodeId, note, starred);
  if (wb.view?.study.id === studyId && view.study.revision >= wb.view.study.revision) setView(view);
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

export function placementForEditor(): string {
  const fen = displayFen.value;
  return fen ? placementOf(fen) : "";
}
