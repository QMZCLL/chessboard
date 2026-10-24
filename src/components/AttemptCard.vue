<script setup lang="ts">
// 先想后讲：展示用户试走分支的讲解，与实际着法对照。
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { NButton, NSpin, NTag } from "naive-ui";
import { api, errorMessage, on } from "../api/backend";
import type { ExplanationEvent, ExplanationRecord } from "../api/types";
import { openComparison } from "../state/compare";
import { newestRecord, nodeOf, selectNode, selectedNode, study } from "../state/workbench";
import VerdictTag from "./VerdictTag.vue";

const props = defineProps<{ nodeId: number }>();

// 与实际着法对比：两步都从实际着法落子前的局面走出
const attemptNode = computed(() => nodeOf(props.nodeId));
const canCompare = computed(() => !!attemptNode.value?.mv && !!selectedNode.value?.mv && attemptNode.value.parent !== null);
function compareAttempt() {
  const a = attemptNode.value;
  const actual = selectedNode.value;
  if (a?.mv && a.parent !== null && actual?.mv) void openComparison(a.parent, a.mv, actual.mv);
}
const record = ref<ExplanationRecord | null>(null);
const notation = ref("");
const error = ref<string | null>(null);

async function load() {
  const s = study.value;
  if (!s) return;
  try {
    const detail = await api.nodeDetail(s.id, props.nodeId);
    notation.value = detail.node.notation ?? "";
    record.value = newestRecord(record.value, detail.explanation);
    if (!detail.explanation?.explanation || detail.explanation.explanation.level !== "detailed") {
      const r = await api.explainMove(s.id, props.nodeId, "quick");
      record.value = newestRecord(record.value, r);
    }
  } catch (e) {
    error.value = errorMessage(e);
  }
}

const off = on<ExplanationEvent>("explanation_updated", (p) => {
  if (p.study_id === study.value?.id && p.node_id === props.nodeId) record.value = newestRecord(record.value, p.record);
});
onBeforeUnmount(off);
watch(
  () => props.nodeId,
  () => {
    record.value = null;
    void load();
  },
  { immediate: true },
);
</script>

<template>
  <section class="attempt" data-testid="attempt-card">
    <header class="head">
      <n-tag size="small" type="info" :bordered="false" round>你的走法</n-tag>
      <strong class="notation">{{ notation }}</strong>
      <VerdictTag v-if="record?.explanation?.verdict" :verdict="record.explanation.verdict" />
      <span class="spacer" />
      <n-button size="small" type="primary" secondary data-testid="compare-attempt" :disabled="!canCompare" @click="compareAttempt">对比我的走法</n-button>
      <n-button size="small" quaternary @click="selectNode(nodeId)">打开分支</n-button>
    </header>
    <p v-if="error" class="error">{{ error }}</p>
    <template v-else-if="record?.explanation">
      <p class="summary">{{ record.explanation.summary }}</p>
      <ul v-if="record.explanation.comparison.length" class="lines">
        <li v-for="c in record.explanation.comparison" :key="c.text">{{ c.text }}</li>
      </ul>
    </template>
    <n-spin v-else size="small" />
    <p v-if="record?.pending" class="muted">正在分析你的走法与候选的差别…</p>
  </section>
</template>

<style scoped>
/* 与讲解中的其他卡片一致：浅色凸起底、细描边、圆角 */
.attempt {
  margin-bottom: var(--sp-4);
  padding: var(--sp-3) var(--sp-4);
  border-radius: var(--radius-lg);
  background: var(--panel-raised);
  box-shadow: inset 0 0 0 1px var(--border);
}
.head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
}
.spacer {
  flex: 1;
}
.notation {
  font-family: var(--piece-font);
  font-size: 18px;
  letter-spacing: 1px;
}
.summary {
  margin: var(--sp-2) 0 var(--sp-1);
  font-family: var(--font-serif);
  font-weight: 600;
  line-height: 1.6;
}
.lines {
  list-style: none;
  margin: 0;
  padding: 0;
}
.lines li {
  position: relative;
  padding-left: 18px;
  font-size: var(--fs-group);
  line-height: 1.7;
  color: var(--text-secondary);
}
.lines li::before {
  content: "";
  position: absolute;
  left: 5px;
  top: calc(0.85em - 2px);
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--text-muted);
}
.muted {
  font-size: var(--fs-aux);
  color: var(--text-muted);
  margin: var(--sp-1) 0 0;
}
.error {
  color: var(--accent-red);
}
</style>
