// 工作台布局：按窗口宽度切换，而不是一味压缩侧栏。
// - wide（≥ 1200px）：棋谱、棋盘、讲解三栏，可拖动栏宽；
// - medium（960～1199px）：棋盘与讲解两栏，棋谱从顶栏按钮展开为浮层；
// - narrow（< 960px）：棋盘在上，下方用标签切换讲解与棋谱。

import { computed, reactive, ref } from "vue";

export const viewport = ref(typeof window !== "undefined" ? window.innerWidth : 1440);
if (typeof window !== "undefined") window.addEventListener("resize", () => (viewport.value = window.innerWidth));

export type Layout = "wide" | "medium" | "narrow";

export const layout = computed<Layout>(() => (viewport.value >= 1200 ? "wide" : viewport.value >= 960 ? "medium" : "narrow"));

export const ui = reactive({
  /** medium 布局下棋谱浮层是否展开 */
  treeOpen: false,
  /** narrow 布局下方显示讲解还是棋谱 */
  narrowTab: "explain" as "explain" | "tree",
});
