// 先想后讲：落子前先自己选择，分级提示与练习记录；尚未揭示的着法不透露。

import { computed, watch } from "vue";
import { api, errorMessage } from "../api/backend";
import type * as T from "../api/types";
import { autoCoach } from "./coach";
import { nodeOf, notify, selectedNode, study, thinking, wb } from "./core";
import { snapshotHints, usedHints } from "./hintSnapshot";
import { setView } from "./studyView";

/**
 * 先想后讲的训练状态：当前局面之后（含正在思考的这一着）尚未揭示的着法都不能透露，
 * 即使当前这一着已经揭示，下一着也要等用户思考后才显示。
 * 棋谱、讲解面板的标签与棋盘标记都按它隐藏答案（与是否正在演示无关）。
 */
export const concealed = computed<Set<number>>(() => {
  const node = selectedNode.value;
  const hidden = new Set<number>();
  if (wb.mode !== "think" || !node) return hidden;
  const visit = (id: number) => {
    const n = nodeOf(id);
    if (!n) return;
    if (n.mv && !wb.revealed[id]) hidden.add(id);
    n.children.forEach(visit);
  };
  visit(node.id);
  return hidden;
});

/**
 * 着法的显示名称。先想后讲中尚未揭示的着法不显示名称：棋谱、批量生成的列表、确认框等
 * 所有显示着法名称的地方都用它，不直接读取 notation。
 */
export function shownNotation(id: number, hidden = "？？？"): string {
  return concealed.value.has(id) ? hidden : (nodeOf(id)?.notation ?? "");
}

/** 正在练习的这一着：属于当前会话、正是选中的着法时才有效（节点编号只在同一个会话内有意义）。 */
export const practiceSession = computed(() => {
  const session = wb.thinkSession;
  return session && session.studyId === study.value?.id && session.nodeId === wb.selected ? session : null;
});

/**
 * 选中这一着的落子前局面详情：属于当前会话、正是这一着的起点时才采用。
 * 切换会话或着法之后，新的起点加载完成之前为空，提示与可走的着法都等它加载。
 */
export const thinkStart = computed(() => {
  const base = wb.thinkBase;
  const node = selectedNode.value;
  return base && node && base.study_id === study.value?.id && base.node.id === node.parent ? base : null;
});

/**
 * 当前局面的分级提示：打开第一级之后显示当时记下的快照（见 hintSnapshot.ts），
 * 之前显示落子前局面详情中的提示（只用来判断有没有提示）。
 */
export const thinkHints = computed(() => {
  if (!thinking.value) return null;
  return practiceSession.value?.shown ?? thinkStart.value?.hints ?? null;
});

/** 二级提示在棋盘上标出的棋子与关系，三级提示中选看的着法。 */
export const thinkMarks = computed(() => {
  const h = thinkHints.value;
  const session = practiceSession.value;
  if (!h || !session || session.hints < 2) return null;
  return { squares: h.focus.squares, arrows: h.focus.arrows, next: session.arrow };
});

// 换到另一着（或另一个会话中的同一编号）思考时开始新的一次练习
watch(
  () => (thinking.value && study.value ? `${study.value.id}/${wb.selected}` : null),
  (key) => {
    const s = study.value;
    if (key && s && !practiceSession.value) wb.thinkSession = { studyId: s.id, nodeId: wb.selected, hints: 0, arrow: null, shown: null };
  },
);

/** 再给一级提示（最多三级）。 */
export function showHint() {
  const session = practiceSession.value;
  if (!session) return;
  // 第一次打开提示时记下显示的内容，之后的各级提示与练习记录都用这份内容。
  // 起点详情还没有加载（或仍是上一个局面的）时不记，提示按钮也要等它加载完成才出现
  if (!session.shown) {
    const start = thinkStart.value;
    if (!start) return;
    session.shown = snapshotHints(start.hints);
  }
  if (session.shown) session.hints = Math.min(3, session.hints + 1);
}

/** 三级提示：在棋盘上标出（或取消标出）一步可以考虑的着法，由用户自己走出来。 */
export function toggleHintMove(mv: T.Move) {
  const session = practiceSession.value;
  if (session) session.arrow = session.arrow === mv ? null : mv;
}

/** 记录这一着的练习：没有走子就看讲解时 firstChoice 为空。只在这一着正在练习时记录。 */
export async function recordPractice(nodeId: number, firstChoice: T.Move | null) {
  const s = study.value;
  const session = wb.thinkSession;
  if (!s || !session || session.studyId !== s.id || session.nodeId !== nodeId) return;
  wb.thinkSession = null;
  try {
    const shown = usedHints(session.shown, session.hints);
    const view = await api.recordPractice(s.id, nodeId, session.hints, firstChoice === null, firstChoice, shown);
    setView(view);
  } catch (e) {
    notify("error", errorMessage(e));
  }
}

let thinkSeq = 0;

export async function loadThinkBase() {
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
  // 没有走子就看讲解：记为直接揭示
  if (!wb.revealed[id]) void recordPractice(id, null);
  wb.revealed[id] = true;
  if (id === wb.selected) autoCoach(wb.coach[id] ?? null);
}

export async function setMode(mode: "auto" | "think") {
  wb.mode = mode;
  if (thinking.value) await loadThinkBase();
}
