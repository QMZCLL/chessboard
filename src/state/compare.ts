// 试走对比：同一局面下“我的选择”与“对照选择”。界面只决定比较起点与两步着法，
// 规则事实、对方应对、评分与差别结论都来自后台（同一次搜索，见 xq-app compare.rs）。
import { computed, reactive, watch } from "vue";
import { api, errorMessage, on } from "../api/backend";
import type * as T from "../api/types";
import { exitPreview, nodeOf, previewFrom, selectedNode, study, wb } from "./workbench";

interface CompareRequest {
  studyId: string;
  /** 比较起点 */
  origin: number;
  mine: T.Move;
  /** 对照着法；为空时由后台选择，收到结果后记下实际使用的着法 */
  reference: T.Move | null;
}

export const cmp = reactive({
  request: null as CompareRequest | null,
  view: null as T.CompareView | null,
  loading: false,
  error: null as string | null,
});

let seq = 0;
let subscribed = false;

const keyOf = (v: { study_id: string; node_id: number; mine: T.Move; reference: T.Move }) => `${v.study_id}:${v.node_id}:${v.mine}:${v.reference}`;

function subscribe() {
  if (subscribed) return;
  subscribed = true;
  on<T.CompareView>("comparison_updated", (v) => {
    const r = cmp.request;
    if (!r || v.study_id !== r.studyId || v.node_id !== r.origin || v.mine !== r.mine) return;
    if (r.reference !== null && v.reference !== r.reference) return;
    cmp.view = v;
  });
}

/** 打开对比：两步棋都从比较起点 origin 走出。 */
export async function openComparison(origin: number, mine: T.Move, reference: T.Move | null = null) {
  const s = study.value;
  if (!s) return;
  subscribe();
  const id = ++seq;
  const sameChoice = cmp.request?.studyId === s.id && cmp.request.origin === origin && cmp.request.mine === mine;
  cmp.request = { studyId: s.id, origin, mine, reference };
  // 只换对照时保留当前内容直到新结果到达，换了比较对象时清空
  if (!sameChoice) cmp.view = null;
  cmp.loading = true;
  cmp.error = null;
  try {
    const v = await api.compareMoves(s.id, origin, mine, reference);
    if (id !== seq || !cmp.request) return;
    cmp.request.reference = v.reference;
    // 推送的完整结果可能先于这次响应到达，不用尚未完成的结果覆盖它
    if (!(cmp.view && keyOf(cmp.view) === keyOf(v) && !cmp.view.pending)) cmp.view = v;
  } catch (e) {
    if (id === seq) cmp.error = errorMessage(e);
  } finally {
    if (id === seq) cmp.loading = false;
  }
}

export function setReference(mv: T.Move) {
  const r = cmp.request;
  if (r && mv !== r.reference) void openComparison(r.origin, r.mine, mv);
}

export function closeComparison() {
  seq += 1;
  if (wb.preview?.source?.startsWith("compare:")) exitPreview();
  cmp.request = null;
  cmp.view = null;
  cmp.error = null;
  cmp.loading = false;
}

/** 从比较起点演示某一方的变化；在两条变化之间切换时保留步数，便于对照同一时刻的局面。 */
export function demoSide(which: "mine" | "reference") {
  const v = cmp.view;
  const side = v?.comparison[which];
  if (!v || !side?.line) return;
  previewFrom({
    title: `${which === "mine" ? "我的选择" : "对照选择"} ${side.notation}（从比较起点）`,
    source: `compare:${which}`,
    baseNode: v.node_id,
    moves: side.line.moves,
    notation: side.line.notation,
    keepIndexWithin: "compare:",
  });
}

/** 试走中：当前着法所在试走分支的第一着就是“我的选择”，起点是开始试走的局面。 */
export const trialChoice = computed(() => {
  const t = wb.trial;
  if (!t) return null;
  for (let cur = selectedNode.value; cur && cur.parent !== null; cur = nodeOf(cur.parent)) {
    if (cur.parent === t.origin && t.roots.includes(cur.id) && cur.mv) return { origin: t.origin, mine: cur.mv };
  }
  return null;
});

// 节点编号只在同一个会话内有意义：切换会话时关闭对比
watch(
  () => study.value?.id,
  (id) => {
    if (cmp.request && cmp.request.studyId !== id) closeComparison();
  },
);
