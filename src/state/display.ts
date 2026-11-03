// 棋盘当前显示什么：演示中的局面、先想后讲的落子前局面或节点本身的局面，以及棋盘上方的状态说明。

import { computed } from "vue";
import type * as T from "../api/types";
import { applyLine, moveNumber, placementOf, sideLabel } from "../utils/board";
import { nodeOf, selectedNode, thinking, wb } from "./core";
import { thinkStart } from "./think";

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
  if (thinking.value) return thinkStart.value?.legal_moves ?? [];
  return wb.detail?.node.id === wb.selected ? wb.detail.legal_moves : [];
});

export const displaySide = computed<T.Color | null>(() => {
  const fen = displayFen.value;
  if (!fen) return null;
  return fen.split(" ")[1] === "b" ? "black" : "red";
});

/** 某个节点之后下一着的回合数。 */
export function roundAfter(node: T.StudyNode): number {
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

export function placementForEditor(): string {
  const fen = displayFen.value;
  return fen ? placementOf(fen) : "";
}
