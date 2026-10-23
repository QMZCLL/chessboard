<script setup lang="ts">
// 棋谱树：当前位置始终可见；“返回主线”回到分支起点，“回到最新局面”到实际对局或主线的最后一着。
import { computed, nextTick, ref, watch } from "vue";
import { NButton, NScrollbar, NTag, NTooltip } from "naive-ui";
import Icon from "./Icon.vue";
import MoveLine from "./MoveLine.vue";
import { backToMainline, latestNode, mainline, nodeOf, selectedEnded, selectNode, study, wb } from "../state/workbench";
import { sideLabel } from "../utils/board";

const root = computed(() => nodeOf(0));
const stats = computed(() => wb.view?.stats);
const onMainline = computed(() => mainline.value.has(wb.selected));
const SOURCE: Record<string, string> = {
  standard: "标准开局",
  setup: "摆棋",
  fen: "FEN 导入",
  record: "棋谱导入",
  capture: "屏幕连接",
};

// 选中的着法变化时滚动到它；后台更新但选择不变时不打断阅读
const body = ref<HTMLElement | null>(null);
watch(
  () => wb.selected,
  async (id) => {
    await nextTick();
    body.value?.querySelector(`[data-node="${id}"]`)?.scrollIntoView({ block: "nearest" });
  },
);
</script>

<template>
  <section class="tree">
    <header>
      <div class="title">
        <span>棋谱</span>
        <n-tag v-if="study" size="small" :bordered="false">{{ SOURCE[study.source] }}</n-tag>
      </div>
      <n-tooltip v-if="stats" placement="bottom">
        <template #trigger>
          <div class="stats" tabindex="0">
            讲解 {{ stats.detailed }}/{{ stats.moves }}
            <span v-if="stats.pending" class="pending">· 进行中 {{ stats.pending }}</span>
            <span v-if="stats.partial + stats.failed" class="failed">· 不完整 {{ stats.partial + stats.failed }}</span>
          </div>
        </template>
        <div>共 {{ stats.moves }} 着：引擎分析完整 {{ stats.detailed }}，至少有规则讲解 {{ stats.basic }}，
          正在分析 {{ stats.pending }}，部分完成可重试 {{ stats.partial }}，无法生成 {{ stats.failed }}</div>
        <div class="legend">
          <span><i class="dot detailed" /> 引擎分析完整</span>
          <span><i class="dot basic" /> 规则讲解</span>
          <span><i class="dot partial" /> 不完整，可重试</span>
          <span><i class="dot queued" /> 排队中</span>
          <span>★ 收藏 · ✎ 笔记</span>
        </div>
      </n-tooltip>
    </header>
    <div v-if="wb.view" class="nav">
      <n-button size="tiny" quaternary :disabled="onMainline" @click="backToMainline">
        <template #icon><Icon name="home" :size="13" /></template>返回主线
      </n-button>
      <n-button size="tiny" quaternary :disabled="latestNode === null || latestNode === wb.selected" @click="latestNode !== null && selectNode(latestNode)">
        <template #icon><Icon name="latest" :size="13" /></template>回到最新局面
      </n-button>
    </div>
    <n-scrollbar class="body">
      <div ref="body">
        <div
          v-if="root"
          class="root focusable"
          :class="{ active: wb.selected === 0 }"
          data-node="0"
          tabindex="0"
          role="button"
          @click="selectNode(0)"
          @keydown.enter.self.prevent="selectNode(0)"
          @keydown.space.self.prevent="selectNode(0)"
        >
          <span>起始局面 · {{ sideLabel(root.side_to_move) }}先走</span>
          <n-tag v-if="study && !study.history_complete" size="tiny" type="warning" :bordered="false">此前历史未知</n-tag>
        </div>
        <MoveLine v-if="root && root.children.length" :start="root.children[0]" :depth="0" />
        <MoveLine v-for="alt in root?.children.slice(1) ?? []" :key="'root-alt' + alt" :start="alt" :depth="1" />
        <p v-if="root && !root.children.length && !selectedEnded" class="hint">在棋盘上走子即可开始记录；每一着都会生成讲解。</p>
      </div>
    </n-scrollbar>
  </section>
</template>

<style scoped>
.tree {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--sp-4) var(--sp-4) var(--sp-1);
  gap: var(--sp-2);
}
.title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-family: var(--font-serif);
  font-weight: 700;
  font-size: 16px;
  letter-spacing: 0.1em;
}
.title :deep(.n-tag) {
  font-family: var(--font-sans);
  letter-spacing: 0;
}
.stats {
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}
.pending {
  color: var(--accent-blue);
}
.failed {
  color: var(--accent-amber);
}
.legend {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  margin-top: 6px;
}
.legend .dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-right: 2px;
}
.legend .dot.detailed {
  background: var(--accent-green);
}
.legend .dot.basic {
  background: var(--accent-blue);
  opacity: 0.6;
}
.legend .dot.partial {
  border: 2px solid var(--accent-amber);
}
.legend .dot.queued {
  border: 1.5px solid currentColor;
}
.nav {
  display: flex;
  gap: var(--sp-1);
  padding: 0 var(--sp-3) var(--sp-2);
  margin-bottom: var(--sp-1);
  border-bottom: 1px solid var(--border);
}
.body {
  flex: 1;
  min-height: 0;
  padding: 0 var(--sp-2) var(--sp-3);
}
.root {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  margin-bottom: 2px;
  border-radius: var(--radius);
  cursor: pointer;
  color: var(--text-secondary);
  font-size: var(--fs-group);
}
.root:hover {
  background: var(--hover-bg);
}
.root.active {
  background: var(--selected-bg);
  box-shadow: inset 3px 0 0 var(--accent-blue);
  color: var(--text-primary);
  font-weight: 600;
}
.hint {
  margin: var(--sp-3) 6px;
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
</style>
