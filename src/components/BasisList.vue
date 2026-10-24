<script setup lang="ts">
// 程序给出的依据原文，紧跟在它支持的那段文字下方。依据是理解结论的关键，保持可读的字号与对比度。
defineProps<{ items: string[] }>();
</script>

<template>
  <ul v-if="items.length" class="basis" data-testid="coach-basis">
    <li v-for="(b, i) in items" :key="i"><span class="label">依据</span>{{ b }}</li>
  </ul>
</template>

<style scoped>
.basis {
  list-style: none;
  margin: 2px 0 var(--sp-2) 26px;
  padding: 6px var(--sp-3);
  border-left: 3px solid var(--border-strong);
  border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  background: var(--tag-gray-bg);
  font-size: var(--fs-basis);
  line-height: 1.6;
  color: var(--text-secondary);
}
.basis li + li {
  margin-top: 2px;
}
.label {
  margin-right: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
}
</style>
