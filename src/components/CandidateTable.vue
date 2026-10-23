<script setup lang="ts">
import type { CandidateView } from "../api/types";
import { showCandidate, wb } from "../state/workbench";
import { scoreText } from "../utils/board";

defineProps<{ candidates: CandidateView[]; base: "parent" | "node"; showActual?: boolean }>();

function previewing(c: CandidateView) {
  return wb.preview?.label === `${c.notation} 的变化`;
}
</script>

<template>
  <div class="candidates">
    <div
      v-for="c in candidates"
      :key="c.mv + (c.rank ?? 'x')"
      class="row"
      :class="{ actual: showActual && c.is_actual, previewing: previewing(c) }"
      :title="c.pv_notation.join(' ')"
      @click="showCandidate(c, base)"
    >
      <span class="rank">{{ c.rank ?? "—" }}</span>
      <span class="notation">{{ c.notation }}</span>
      <span class="score" :class="{ bound: c.bound !== 'exact' }">
        {{ scoreText(c.score) }}<template v-if="c.bound === 'lower'">↑</template><template v-else-if="c.bound === 'upper'">↓</template>
      </span>
      <span class="summary">{{ c.summary }}</span>
      <span v-if="showActual && c.is_actual" class="tag">实际着法</span>
      <span class="pv">{{ c.pv_notation.slice(1, 5).join(" ") }}</span>
    </div>
    <p v-if="!candidates.length" class="empty">暂无候选</p>
  </div>
</template>

<style scoped>
.candidates {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.row {
  display: grid;
  grid-template-columns: 1.6em 5.5em 5em 1fr auto;
  grid-template-rows: auto auto;
  column-gap: 8px;
  align-items: baseline;
  padding: 6px 8px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 14px;
}
.row:hover {
  background: var(--hover-bg);
}
.row.previewing {
  background: var(--tag-blue-bg);
}
.row.actual .notation {
  text-decoration: underline;
  text-decoration-color: var(--accent-amber);
  text-underline-offset: 3px;
}
.rank {
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}
.notation {
  font-family: var(--piece-font);
  font-weight: 600;
}
.score {
  font-variant-numeric: tabular-nums;
  color: var(--text-secondary);
}
.score.bound {
  font-style: italic;
}
.summary {
  color: var(--text-secondary);
  font-size: 13px;
}
.tag {
  font-size: 11px;
  padding: 0 5px;
  border-radius: 4px;
  background: var(--highlight-bg);
  color: var(--accent-amber);
}
.pv {
  grid-column: 2 / -1;
  font-size: 12px;
  color: var(--text-muted);
  font-family: var(--piece-font);
}
.empty {
  color: var(--text-muted);
  font-size: 13px;
  margin: 4px 8px;
}
</style>
