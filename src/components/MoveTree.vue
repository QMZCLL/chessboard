<script setup lang="ts">
import { computed } from "vue";
import { NScrollbar, NTag, NTooltip } from "naive-ui";
import MoveLine from "./MoveLine.vue";
import { nodeOf, selectNode, study, wb } from "../state/workbench";
import { sideLabel } from "../utils/board";

const root = computed(() => nodeOf(0));
const stats = computed(() => wb.view?.stats);
const SOURCE: Record<string, string> = {
  standard: "标准开局",
  setup: "摆棋",
  fen: "FEN 导入",
  record: "棋谱导入",
  capture: "屏幕连接",
};
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
          <div class="stats">
            讲解 {{ stats.detailed }}/{{ stats.moves }}
            <span v-if="stats.pending" class="pending">· 进行中 {{ stats.pending }}</span>
            <span v-if="stats.failed" class="failed">· 未完成 {{ stats.failed }}</span>
          </div>
        </template>
        共 {{ stats.moves }} 着：详细讲解 {{ stats.detailed }}，至少有基本讲解 {{ stats.basic }}，
        正在分析 {{ stats.pending }}，详细讲解未完成 {{ stats.failed }}
      </n-tooltip>
    </header>
    <n-scrollbar class="body">
      <div v-if="root" class="root" :class="{ active: wb.selected === 0 }" data-node="0" @click="selectNode(0)">
        <span>起始局面 · {{ sideLabel(root.side_to_move) }}先走</span>
        <n-tag v-if="study && !study.history_complete" size="tiny" type="warning" :bordered="false">此前历史未知</n-tag>
      </div>
      <MoveLine v-if="root && root.children.length" :start="root.children[0]" :depth="0" />
      <MoveLine v-for="alt in root?.children.slice(1) ?? []" :key="'root-alt' + alt" :start="alt" :depth="1" />
      <p v-if="root && !root.children.length" class="hint">在棋盘上走子即可开始记录；每一着都会生成讲解。</p>
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
  padding: 10px 12px 6px;
  gap: 8px;
}
.title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 600;
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
.body {
  flex: 1;
  min-height: 0;
  padding: 0 8px 12px;
}
.root {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 6px;
  margin-bottom: 2px;
  border-radius: 6px;
  cursor: pointer;
  color: var(--text-secondary);
  font-size: 14px;
}
.root:hover {
  background: var(--hover-bg);
}
.root.active {
  background: var(--selected-bg);
  color: var(--text-primary);
  font-weight: 600;
}
.hint {
  margin: 12px 6px;
  color: var(--text-muted);
  font-size: 13px;
}
</style>
