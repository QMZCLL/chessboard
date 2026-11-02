<script setup lang="ts">
// 搜索结论的稳定性：标签写明等级，依据逐条列出。“仍在变化”时讲解与比较都不评价排名与优劣。
import { computed } from "vue";
import { NTag, NTooltip } from "naive-ui";
import type { Stability } from "../api/types";
import { STABILITY } from "../state/stability";

const props = defineProps<{ stability: Stability; inline?: boolean }>();
const info = computed(() => STABILITY[props.stability.level]);
</script>

<template>
  <!-- 标题旁的标签：依据放在提示中 -->
  <n-tooltip v-if="!inline" placement="bottom">
    <template #trigger>
      <n-tag size="small" :bordered="false" :type="info.type" data-testid="stability-tag">{{ info.text }}</n-tag>
    </template>
    <div v-for="r in stability.reasons" :key="r">{{ r }}</div>
  </n-tooltip>
  <!-- 候选比较下的说明：依据直接写出 -->
  <p v-else class="stability" :class="stability.level" data-testid="stability-note">
    <strong>结论稳定性：{{ info.text }}</strong>
    <span v-for="r in stability.reasons" :key="r" class="reason">{{ r }}</span>
    <span v-if="stability.level === 'shifting'" class="reason">排名还在变化时不评价哪一着更好；深入分析可以增加搜索深度。</span>
  </p>
</template>

<style scoped>
.stability {
  margin: 6px 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-muted);
}
.stability strong {
  color: var(--text-primary);
  font-weight: 600;
  margin-right: 6px;
}
.stability.shifting strong {
  color: var(--accent-amber);
}
.reason {
  display: block;
}
</style>
