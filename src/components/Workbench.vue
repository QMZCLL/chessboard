<script setup lang="ts">
// 学习工作台：按窗口宽度切换布局（见 state/layout.ts）。宽屏时三栏宽度可拖动调整。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { NButton, NResult, NSpin, NTabPane, NTabs, useMessage } from "naive-ui";
import BoardArea from "./BoardArea.vue";
import CaptureDialog from "./CaptureDialog.vue";
import ExplanationPanel from "./ExplanationPanel.vue";
import MoveTree from "./MoveTree.vue";
import NewStudyDialog from "./NewStudyDialog.vue";
import SettingsDrawer from "./SettingsDrawer.vue";
import StatusBar from "./StatusBar.vue";
import TopBar from "./TopBar.vue";
import { boot, setNotifier, study, wb } from "../state/workbench";
import { layout, ui, viewport } from "../state/layout";

const message = useMessage();
setNotifier((type, text) => message[type](text, { duration: type === "error" ? 6000 : 3500 }));

function stored(key: string, fallback: number) {
  try {
    const v = Number(localStorage.getItem(key));
    return Number.isFinite(v) && v > 0 ? v : fallback;
  } catch {
    return fallback;
  }
}

const left = ref(stored("xq-left", 260));
const right = ref(stored("xq-right", 420));

// 宽屏三栏：中间棋盘至少保留 420px；栏间留出与外边距相同的空隙（兼作拖动条）
const CENTER_MIN = 420;
const GAP = 10;
const CHROME = GAP * 4;
const effectiveLeft = computed(() => Math.max(200, Math.min(left.value, (viewport.value - CHROME - CENTER_MIN) * 0.3)));
const effectiveRight = computed(() =>
  Math.max(340, Math.min(right.value, viewport.value - CHROME - CENTER_MIN - (layout.value === "wide" ? effectiveLeft.value : 0))),
);
const columns = computed(() =>
  layout.value === "wide"
    ? `${effectiveLeft.value}px ${GAP}px minmax(${CENTER_MIN}px, 1fr) ${GAP}px ${effectiveRight.value}px`
    : `minmax(${CENTER_MIN}px, 1fr) ${GAP}px ${effectiveRight.value}px`,
);
let resizing: { side: "left" | "right"; x: number; start: number } | null = null;

function startResize(side: "left" | "right", e: MouseEvent) {
  resizing = { side, x: e.clientX, start: side === "left" ? effectiveLeft.value : effectiveRight.value };
  window.addEventListener("mousemove", onResize);
  window.addEventListener("mouseup", stopResize);
  e.preventDefault();
}

function onResize(e: MouseEvent) {
  if (!resizing) return;
  const dx = e.clientX - resizing.x;
  if (resizing.side === "left") left.value = Math.min(480, Math.max(200, resizing.start + dx));
  else right.value = Math.min(720, Math.max(340, resizing.start - dx));
}

function stopResize() {
  resizing = null;
  window.removeEventListener("mousemove", onResize);
  window.removeEventListener("mouseup", stopResize);
  try {
    localStorage.setItem("xq-left", String(left.value));
    localStorage.setItem("xq-right", String(right.value));
  } catch {
    // 记不住宽度不影响使用
  }
}

onMounted(() => void boot());
onBeforeUnmount(stopResize);
</script>

<template>
  <div v-if="!wb.booted" class="boot"><n-spin /> 正在启动…</div>
  <n-result v-else-if="wb.startupError" status="error" title="服务启动失败" :description="wb.startupError" class="boot">
    <template #footer>
      <p class="muted">可能是另一个实例正在运行，或数据目录不可写。关闭其他实例后重新打开应用。</p>
    </template>
  </n-result>
  <div v-else class="shell" :class="layout">
    <TopBar />
    <!-- 宽屏与中等宽度：分栏 -->
    <main v-if="!study" class="columns solo">
      <section class="col center">
        <div class="empty">
          <div class="seal-large" aria-hidden="true">弈</div>
          <p class="empty-title">还没有打开学习局面</p>
          <p class="muted">导入棋谱、摆一个残局，或从标准开局开始。</p>
          <n-button type="primary" @click="wb.dialogs.newStudy = true">建立学习局面</n-button>
        </div>
      </section>
    </main>
    <main v-else-if="layout !== 'narrow'" class="columns" :style="{ gridTemplateColumns: columns }">
      <template v-if="layout === 'wide'">
        <aside class="col surface" aria-label="棋谱"><MoveTree /></aside>
        <div class="splitter" role="separator" aria-orientation="vertical" @mousedown="startResize('left', $event)" />
      </template>
      <section class="col center">
        <BoardArea />
        <!-- 中等宽度：棋谱从顶栏按钮展开为浮层 -->
        <template v-if="layout === 'medium' && ui.treeOpen">
          <div class="backdrop" @click="ui.treeOpen = false" />
          <aside class="tree-overlay surface" aria-label="棋谱" data-testid="tree-overlay"><MoveTree /></aside>
        </template>
      </section>
      <div class="splitter" role="separator" aria-orientation="vertical" @mousedown="startResize('right', $event)" />
      <aside class="col surface" aria-label="讲解"><ExplanationPanel /></aside>
    </main>
    <!-- 窄窗口：棋盘在上，讲解与棋谱用标签切换 -->
    <main v-else class="stack">
      <section class="stack-board">
        <BoardArea />
      </section>
      <section class="stack-panel surface">
        <n-tabs v-model:value="ui.narrowTab" type="segment" size="small" class="stack-tabs">
          <n-tab-pane name="explain" tab="讲解" display-directive="show"><ExplanationPanel /></n-tab-pane>
          <n-tab-pane name="tree" tab="棋谱" display-directive="show"><MoveTree /></n-tab-pane>
        </n-tabs>
      </section>
    </main>
    <StatusBar />
    <NewStudyDialog />
    <CaptureDialog />
    <SettingsDrawer />
  </div>
</template>

<style scoped>
.shell {
  height: 100vh;
  display: flex;
  flex-direction: column;
}
.columns {
  flex: 1;
  min-height: 0;
  display: grid;
  padding: var(--gutter);
}
.columns.solo {
  grid-template-columns: 1fr;
}
.col {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}
.col.center {
  position: relative;
}
/* 栏间空隙兼作拖动条：悬停时显示一条细线 */
.splitter {
  cursor: col-resize;
  position: relative;
}
.splitter::after {
  content: "";
  position: absolute;
  top: 30%;
  bottom: 30%;
  left: 50%;
  width: 2px;
  margin-left: -1px;
  border-radius: 1px;
  background: transparent;
  transition: background-color 0.15s ease;
}
.splitter:hover::after {
  background: var(--border-strong);
}
.backdrop {
  position: absolute;
  inset: 0;
  background: rgba(30, 20, 5, 0.12);
  backdrop-filter: blur(1px);
  z-index: 5;
}
.tree-overlay {
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  width: min(320px, 80%);
  box-shadow: var(--shadow-3);
  z-index: 6;
}
.stack {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.stack-board {
  height: min(62vh, calc(100vw * 672 / 612 + 110px));
  min-height: 320px;
  flex-shrink: 0;
}
.stack-panel {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  margin: 0 var(--sp-2) var(--sp-2);
}
.stack-tabs {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.stack-tabs :deep(.n-tabs-nav) {
  padding: var(--sp-2) var(--sp-3) 0;
}
.stack-tabs :deep(.n-tab-pane) {
  flex: 1;
  min-height: 0;
  padding: 0;
}
.empty {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sp-2);
  color: var(--text-muted);
}
.empty p {
  margin: 0;
}
.empty .n-button {
  margin-top: var(--sp-3);
}
.empty-title {
  font-family: var(--font-serif);
  font-size: 20px;
  color: var(--text-primary);
}
.seal-large {
  width: 64px;
  height: 64px;
  display: grid;
  place-items: center;
  margin-bottom: var(--sp-2);
  border-radius: 10px;
  background: var(--seal);
  color: #fff6ee;
  font-family: var(--piece-font);
  font-size: 40px;
  box-shadow: inset 0 0 0 3px rgba(255, 255, 255, 0.18), var(--shadow-2);
}
.boot {
  height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sp-3);
}
.muted {
  color: var(--text-muted);
}
</style>
