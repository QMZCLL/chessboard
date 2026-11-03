// 学习工作台的共享状态：棋盘、棋谱、讲解面板都从这里取数据，避免各组件直接改动 DOM。
// 各部分的操作分在同目录的几个文件中（studyView、coach、think、display、navigation、preview、session），
// 对外统一从 workbench.ts 导出。

import { computed, reactive } from "vue";
import { api, errorMessage } from "../api/backend";
import type * as T from "../api/types";

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

let notifier: Notify = (type, text) => console.log(type, text);

/** 在界面上提示一条消息（由 App 通过 setNotifier 接上） */
export function notify(type: Parameters<Notify>[0], text: string) {
  notifier(type, text);
}

export const wb = reactive({
  booted: false,
  startupError: null as string | null,
  status: null as T.AppStatus | null,
  view: null as T.StudyView | null,
  selected: 0,
  detail: null as T.NodeDetail | null,
  detailError: null as string | null,
  /** 先想后讲模式下作为试走起点的局面（可能还是上一着或上一个会话的，使用前见 thinkStart） */
  thinkBase: null as T.NodeDetail | null,
  /** 先想后讲中正在思考的这一着（哪个会话的哪一着）：已经看了几级提示、三级提示中选看的着法、看到的提示 */
  thinkSession: null as { studyId: string; nodeId: number; hints: number; arrow: T.Move | null; shown: T.ThinkHints | null } | null,
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
  /** AI 讲解状态（按节点），来自节点详情与 coach_updated 事件 */
  coach: {} as Record<number, T.CoachView>,
  capture: { state: "idle" } as T.CaptureState,
  followLive: true,
  dialogs: { newStudy: false, capture: false, settings: false, studies: false, coachBatch: false, review: false, report: false },
  /**
   * 主区域：棋盘学习、今日学习首页，或一组练习的专注练习工作区（练习保存在后台，退出后可以继续）。
   * 还没有打开学习局面时显示首页。
   */
  main: "board" as "board" | "home" | "practice",
  /** 下次打开设置时显示的分组（从其他位置跳转到某项设置时使用） */
  settingsTab: null as SettingsTab | null,
});

export type SettingsTab = "learn" | "coach" | "engine" | "capture" | "archive" | "diag";

export function openSettings(tab: SettingsTab | null = null) {
  wb.settingsTab = tab;
  wb.dialogs.settings = true;
}
export function setNotifier(fn: Notify) {
  notifier = fn;
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

export function safeStorage(op: "get" | "set", key: string, value?: string): string | null {
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

/** 主线最后一着 */
export function lastMainline(view: T.StudyView): number {
  let id = 0;
  for (;;) {
    const next = view.study.nodes[String(id)]?.children[0];
    if (next === undefined) return id;
    id = next;
  }
}

const POSITIONS_KEY = "xq-study-positions";
const POSITIONS_KEEP = 20;

/** 每个学习会话上次看到的着法（只在本机记住，最近的若干个） */
export function lastPositions(): Record<string, { node: number; ply: number; at: number }> {
  try {
    const v = JSON.parse(safeStorage("get", POSITIONS_KEY) ?? "{}");
    return v && typeof v === "object" ? v : {};
  } catch {
    return {};
  }
}

export function rememberPosition(studyId: string, node: number, ply: number) {
  const all = { ...lastPositions(), [studyId]: { node, ply, at: Date.now() } };
  const kept = Object.entries(all)
    .sort((a, b) => b[1].at - a[1].at)
    .slice(0, POSITIONS_KEEP);
  safeStorage("set", POSITIONS_KEY, JSON.stringify(Object.fromEntries(kept)));
}
