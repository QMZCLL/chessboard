<script setup lang="ts">
// 候选列表：点击一行选中并展示详情（主变化、首着作用）；“演示”按钮回放该候选的合法变化。
// 候选从落子前局面开始时，不把首着画在落子后的棋盘上。
import { computed } from "vue";
import { NButton } from "naive-ui";
import Icon from "./Icon.vue";
import type { CandidateView } from "../api/types";
import { openComparison } from "../state/compare";
import { candidateSource, focusCandidate, selectedNode, showCandidate, thinking, wb } from "../state/workbench";
import { scoreText } from "../utils/board";

const props = defineProps<{ candidates: CandidateView[]; base: "parent" | "node"; showActual?: boolean; scope: string }>();

function keyOf(c: CandidateView) {
  return `${props.scope}:${c.mv}:${c.rank ?? "x"}`;
}

const focused = computed(() => wb.focus?.key ?? null);
const onBoard = computed(() => props.base === "node" && !wb.preview && !thinking.value);

function previewing(c: CandidateView) {
  return wb.preview?.source === candidateSource(c, props.base);
}
</script>

<template>
  <div class="candidates">
    <div v-for="c in candidates" :key="c.mv + (c.rank ?? 'x')" class="item">
      <div
        class="row focusable"
        :class="{ actual: showActual && c.is_actual, previewing: previewing(c), active: focused === keyOf(c) }"
        tabindex="0"
        role="button"
        :aria-pressed="focused === keyOf(c)"
        @click="focusCandidate(keyOf(c), c, base)"
        @keydown.enter.self.prevent="focusCandidate(keyOf(c), c, base)"
        @keydown.space.self.prevent="focusCandidate(keyOf(c), c, base)"
      >
        <span class="rank">{{ c.rank ?? "—" }}</span>
        <span class="notation">{{ c.notation }}</span>
        <span class="score" :class="{ bound: c.bound !== 'exact' }" :title="c.bound === 'exact' ? '' : '搜索边界值，不是精确评分'" data-testid="candidate-score">
          {{ scoreText(c.score, c.bound) }}
        </span>
        <span class="summary">{{ c.summary }}</span>
        <span v-if="showActual && c.is_actual" class="tag">实际着法</span>
        <n-button v-if="c.pv.length" text size="tiny" type="info" class="demo" @click.stop="showCandidate(c, base)">
          <template #icon><Icon name="play" :size="11" /></template>演示
        </n-button>
      </div>
      <div v-if="focused === keyOf(c)" class="detail">
        <p v-if="c.pv_notation.length" class="pv"><span class="muted">主变化：</span>{{ c.pv_notation.join(" ") }}</p>
        <p v-else class="muted">这条候选没有可演示的合法变化。</p>
        <p v-if="!onBoard" class="muted">候选从落子前的局面开始，点击“演示”查看。</p>
        <n-button
          v-if="showActual && !c.is_actual && selectedNode?.mv && selectedNode.parent !== null"
          size="small"
          secondary
          class="what-if"
          data-testid="compare-candidate"
          @click="openComparison(selectedNode.parent, c.mv, selectedNode.mv)"
        >
          <template #icon><Icon name="compare" /></template>如果改走这步，会怎样？
        </n-button>
      </div>
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
/* 第一行：序号、着法、评分、标记与演示；第二行：首着作用，宽度不受按钮挤压 */
.row {
  display: grid;
  grid-template-columns: 1.6em 5.5em auto 1fr auto;
  grid-template-areas:
    "rank notation score tag demo"
    ". summary summary summary summary";
  column-gap: var(--sp-2);
  align-items: baseline;
  padding: 7px var(--sp-3) 7px var(--sp-2);
  border-radius: var(--radius);
  cursor: pointer;
  font-size: var(--fs-group);
  transition: background-color 0.15s ease, box-shadow 0.15s ease;
}
.row:hover {
  background: var(--hover-bg);
}
.row.active {
  background: var(--selected-bg);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent-blue) 55%, transparent);
}
.row.previewing {
  background: var(--tag-blue-bg);
}
.row.actual .notation {
  text-decoration: underline;
}
.rank {
  grid-area: rank;
  align-self: center;
  justify-self: center;
  width: 20px;
  height: 20px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-muted);
  background: var(--tag-gray-bg);
}
.notation {
  grid-area: notation;
  font-family: var(--piece-font);
  font-weight: 600;
}
.score {
  grid-area: score;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.score.bound {
  color: var(--text-muted);
}
.summary {
  grid-area: summary;
  color: var(--text-secondary);
  font-size: var(--fs-aux);
}
.tag {
  grid-area: tag;
  justify-self: start;
  font-size: 11px;
  padding: 0 4px;
  border-radius: 4px;
  background: var(--tag-amber-bg);
  color: var(--accent-amber);
  white-space: nowrap;
}
.demo {
  grid-area: demo;
  font-size: 12px;
}
.detail {
  padding: 2px var(--sp-2) var(--sp-2) calc(1.6em + var(--sp-3));
  font-size: var(--fs-aux);
}
.detail p {
  margin: 2px 0;
}
.pv {
  font-family: var(--piece-font);
}
.what-if {
  margin-top: var(--sp-1);
}
.muted {
  color: var(--text-muted);
}
.empty {
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
</style>
