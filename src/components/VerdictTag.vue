<script setup lang="ts">
import { computed } from "vue";
import { NTag, NTooltip } from "naive-ui";
import type { Verdict } from "../api/types";

const props = defineProps<{ verdict: Verdict }>();

const type = computed(() => {
  switch (props.verdict.tone) {
    case "best":
      return "success";
    case "close":
      return "info";
    case "inaccuracy":
      return "default";
    case "mistake":
      return "warning";
    case "blunder":
      return "error";
    default:
      return "default";
  }
});

const text = computed(() => {
  const d = props.verdict.delta;
  return d !== null && props.verdict.tone !== "best" ? `${props.verdict.label}（约 ${d} 分）` : props.verdict.label;
});
</script>

<template>
  <n-tooltip v-if="verdict.note" placement="bottom">
    <template #trigger>
      <n-tag :type="type" size="small" :bordered="false">{{ text }}</n-tag>
    </template>
    {{ verdict.note }}
  </n-tooltip>
  <n-tag v-else :type="type" size="small" :bordered="false">{{ text }}</n-tag>
</template>
