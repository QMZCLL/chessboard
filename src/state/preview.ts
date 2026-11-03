// 依据、演示与高亮：选中讲解中的一段、演示依据中的变化与候选、在棋盘上逐着查看，以及把演示的变化保存为分支。

import { computed } from "vue";
import { api, errorMessage } from "../api/backend";
import type * as T from "../api/types";
import { notify, selectedNode, study, thinking, wb, type Preview } from "./core";
import { selectNode } from "./navigation";
import { setView } from "./studyView";

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

/**
 * 从指定局面演示一条变化（如试走对比中，从比较起点演示两种选择）。
 * keepIndexWithin：正在演示的变化来源以此开头时保留当前步数，便于在两条变化之间切换对照。
 */
export function previewFrom(p: { title: string; source: string; baseNode: number; moves: T.Move[]; notation: string[]; keepIndexWithin?: string }) {
  const current = wb.preview;
  const keep = p.keepIndexWithin && current?.source?.startsWith(p.keepIndexWithin) ? Math.min(current.index, p.moves.length) : 0;
  wb.highlight = null;
  wb.preview = { title: p.title, source: p.source, baseNode: p.baseNode, moves: p.moves, notation: p.notation, index: keep, marks: null };
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
