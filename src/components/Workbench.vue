<script setup lang="ts">
// 学习工作台：左侧棋谱、中间棋盘、右侧讲解，三栏宽度可拖动调整。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { NButton, NResult, NSpin, useMessage } from "naive-ui";
import BoardArea from "./BoardArea.vue";
import CaptureDialog from "./CaptureDialog.vue";
import ExplanationPanel from "./ExplanationPanel.vue";
import MoveTree from "./MoveTree.vue";
import NewStudyDialog from "./NewStudyDialog.vue";
import SettingsDrawer from "./SettingsDrawer.vue";
import StatusBar from "./StatusBar.vue";
import TopBar from "./TopBar.vue";
import { boot, setNotifier, study, wb } from "../state/workbench";

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
const viewport = ref(window.innerWidth);
const onWindowResize = () => (viewport.value = window.innerWidth);

// 窗口较窄时两侧栏让出空间，保证中间棋盘至少有 320px
const CENTER_MIN = 320;
const effectiveLeft = computed(() => Math.max(180, Math.min(left.value, (viewport.value - 12 - CENTER_MIN) * 0.35)));
const effectiveRight = computed(() =>
  Math.max(280, Math.min(right.value, viewport.value - 12 - CENTER_MIN - effectiveLeft.value)),
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
  if (resizing.side === "left") left.value = Math.min(480, Math.max(180, resizing.start + dx));
  else right.value = Math.min(720, Math.max(320, resizing.start - dx));
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

onMounted(() => {
  window.addEventListener("resize", onWindowResize);
  void boot();
});
onBeforeUnmount(() => {
  stopResize();
  window.removeEventListener("resize", onWindowResize);
});
</script>

<template>
  <div v-if="!wb.booted" class="boot"><n-spin /> 正在启动…</div>
  <n-result v-else-if="wb.startupError" status="error" title="服务启动失败" :description="wb.startupError" class="boot">
    <template #footer>
      <p class="muted">可能是另一个实例正在运行，或数据目录不可写。关闭其他实例后重新打开应用。</p>
    </template>
  </n-result>
  <div v-else class="shell">
    <TopBar />
    <main class="columns" :style="{ gridTemplateColumns: `${effectiveLeft}px 6px minmax(${CENTER_MIN}px, 1fr) 6px ${effectiveRight}px` }">
      <aside class="col"><MoveTree /></aside>
      <div class="splitter" @mousedown="startResize('left', $event)" />
      <section class="col center">
        <BoardArea v-if="study" />
        <div v-else class="empty">
          <p>还没有打开学习局面。</p>
          <n-button type="primary" @click="wb.dialogs.newStudy = true">建立学习局面</n-button>
        </div>
      </section>
      <div class="splitter" @mousedown="startResize('right', $event)" />
      <aside class="col"><ExplanationPanel v-if="study" /></aside>
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
}
.col {
  min-width: 0;
  min-height: 0;
  background: var(--panel);
  overflow: hidden;
}
.col.center {
  background: var(--bg);
}
.splitter {
  cursor: col-resize;
  background: var(--border);
}
.splitter:hover {
  background: var(--border-strong);
}
.empty {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-muted);
}
.boot {
  height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
}
.muted {
  color: var(--text-muted);
}
</style>
