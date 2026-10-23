<script setup lang="ts">
// 先想后讲：展示用户试走分支的讲解，与实际着法对照。
import { onBeforeUnmount, ref, watch } from "vue";
import { NButton, NSpin, NTag } from "naive-ui";
import { api, errorMessage, on } from "../api/backend";
import type { ExplanationEvent, ExplanationRecord } from "../api/types";
import { newestRecord, selectNode, study } from "../state/workbench";
import VerdictTag from "./VerdictTag.vue";

const props = defineProps<{ nodeId: number }>();
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
  <div class="attempt">
    <div class="head">
      <n-tag size="small" type="info" :bordered="false">你的走法</n-tag>
      <strong class="notation">{{ notation }}</strong>
      <VerdictTag v-if="record?.explanation?.verdict" :verdict="record.explanation.verdict" />
      <n-button size="tiny" quaternary @click="selectNode(nodeId)">打开分支</n-button>
    </div>
    <p v-if="error" class="error">{{ error }}</p>
    <template v-else-if="record?.explanation">
      <p class="summary">{{ record.explanation.summary }}</p>
      <p v-for="c in record.explanation.comparison" :key="c.text" class="line">{{ c.text }}</p>
    </template>
    <n-spin v-else size="small" />
    <p v-if="record?.pending" class="muted">正在分析你的走法与候选的差别…</p>
  </div>
</template>

<style scoped>
.attempt {
  border: 1px solid var(--border-strong);
  border-radius: 8px;
  padding: 10px 12px;
  margin-bottom: 12px;
  background: var(--panel-raised);
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.notation {
  font-family: var(--piece-font);
  font-size: 16px;
}
.summary {
  margin: 6px 0 2px;
}
.line {
  margin: 2px 0;
  font-size: 13px;
  color: var(--text-secondary);
}
.muted {
  font-size: 12px;
  color: var(--text-muted);
  margin: 4px 0 0;
}
.error {
  color: var(--accent-red);
}
</style>
