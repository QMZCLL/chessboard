<script setup lang="ts">
// 中间棋盘区：显示当前节点、试走、变化演示，并提供前进后退与翻转。
import { computed, onBeforeUnmount, onMounted } from "vue";
import { NButton, NButtonGroup, NTag, NTooltip } from "naive-ui";
import XiangqiBoard from "./XiangqiBoard.vue";
import {
  addPreviewAsBranch,
  displayFen,
  displayLastMove,
  displayLegal,
  displaySide,
  exitPreview,
  flipBoard,
  goRelative,
  playMove,
  selectedNode,
  stepPreview,
  study,
  thinking,
  wb,
} from "../state/workbench";
import { sideLabel } from "../utils/board";

const orientation = computed(() => study.value?.orientation ?? "red_bottom");
const detailHere = computed(() => (wb.detail?.node.id === wb.selected ? wb.detail : null));
const status = computed(() => (wb.preview || thinking.value ? null : detailHere.value?.status));
const inCheck = computed(() => !wb.preview && !thinking.value && detailHere.value?.in_check);
const interactive = computed(() => !wb.preview && status.value !== "checkmate" && status.value !== "stalemate");

const previewArrow = computed(() => (wb.preview ? displayLastMove.value : null));
const lastMove = computed(() => (wb.preview ? null : displayLastMove.value));

function onKey(e: KeyboardEvent) {
  const target = e.target as HTMLElement | null;
  if (target && ["INPUT", "TEXTAREA"].includes(target.tagName)) return;
  if (e.key === "ArrowLeft") (wb.preview ? stepPreview(-1) : goRelative(-1), e.preventDefault());
  if (e.key === "ArrowRight") (wb.preview ? stepPreview(1) : goRelative(1), e.preventDefault());
  if (e.key === "Escape") exitPreview();
}
onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <section class="area">
    <div class="banner">
      <template v-if="wb.preview">
        <n-tag type="info" size="small" :bordered="false">演示</n-tag>
        <span class="label">{{ wb.preview.label }}</span>
        <span class="steps">
          <span
            v-for="(n, i) in wb.preview.notation"
            :key="i"
            class="step"
            :class="{ done: i < wb.preview.index, current: i === wb.preview.index - 1 }"
            @click="wb.preview.index = i + 1"
            >{{ n }}</span
          >
        </span>
      </template>
      <template v-else-if="displaySide">
        <span class="side" :class="displaySide">{{ sideLabel(displaySide) }}走</span>
        <n-tag v-if="status === 'checkmate'" type="error" size="small" :bordered="false">已被将死</n-tag>
        <n-tag v-else-if="status === 'stalemate'" type="error" size="small" :bordered="false">困毙（无子可走，判负）</n-tag>
        <n-tag v-else-if="inCheck" type="warning" size="small" :bordered="false">正被将军</n-tag>
        <n-tag v-if="thinking" type="info" size="small" :bordered="false">先想后讲：请走出你的选择</n-tag>
        <n-tag v-if="detailHere && detailHere.repetition > 1" size="small" :bordered="false">局面重复 {{ detailHere.repetition }} 次</n-tag>
      </template>
    </div>

    <div class="board-wrap">
      <XiangqiBoard
        v-if="displayFen"
        :fen="displayFen"
        :orientation="orientation"
        :legal-moves="displayLegal"
        :last-move="lastMove"
        :preview-arrow="previewArrow"
        :highlights="wb.highlight?.squares ?? []"
        :arrows="wb.preview ? [] : wb.highlight?.arrows ?? []"
        :danger="wb.illegal?.squares ?? []"
        :interactive="interactive"
        @move="playMove"
      />
    </div>

    <div v-if="wb.illegal" class="illegal">不能这样走：{{ wb.illegal.message }}</div>

    <div class="controls">
      <template v-if="wb.preview">
        <n-button-group size="small">
          <n-button @click="wb.preview.index = 0">起点</n-button>
          <n-button @click="stepPreview(-1)">后退</n-button>
          <n-button @click="stepPreview(1)">前进</n-button>
          <n-button @click="wb.preview.index = wb.preview.moves.length">终点</n-button>
        </n-button-group>
        <n-button size="small" @click="addPreviewAsBranch">加入为分支</n-button>
        <n-button size="small" type="primary" secondary @click="exitPreview">退出演示</n-button>
      </template>
      <template v-else>
        <n-button-group size="small">
          <n-tooltip>
            <template #trigger><n-button :disabled="!selectedNode || selectedNode.parent === null" @click="goRelative(-1)">上一步</n-button></template>
            键盘 ←
          </n-tooltip>
          <n-tooltip>
            <template #trigger><n-button :disabled="!selectedNode?.children.length" @click="goRelative(1)">下一步</n-button></template>
            键盘 →（沿主线）
          </n-tooltip>
        </n-button-group>
        <n-button size="small" quaternary @click="flipBoard">翻转棋盘</n-button>
      </template>
    </div>
  </section>
</template>

<style scoped>
.area {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  min-height: 0;
  padding: 8px 12px;
  gap: 6px;
}
.banner {
  width: 100%;
  min-height: 30px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.side {
  font-weight: 700;
  padding: 2px 10px;
  border-radius: 12px;
  font-size: 14px;
}
.side.red {
  background: var(--tag-red-bg);
  color: var(--accent-red);
}
.side.black {
  background: var(--tag-gray-bg);
  color: var(--text-primary);
}
.label {
  font-size: 13px;
  color: var(--text-secondary);
}
.steps {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  font-family: var(--piece-font);
}
.step {
  cursor: pointer;
  color: var(--text-muted);
  padding: 0 3px;
  border-radius: 4px;
}
.step.done {
  color: var(--text-primary);
}
.step.current {
  background: var(--tag-blue-bg);
  color: var(--accent-blue);
}
.board-wrap {
  flex: 1;
  min-height: 0;
  width: 100%;
  display: flex;
  justify-content: center;
  aspect-ratio: 612 / 672;
  max-width: calc((100vh - 220px) * 612 / 672);
}
.illegal {
  color: var(--accent-red);
  font-size: 14px;
}
.controls {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  justify-content: center;
}
</style>
