<script setup lang="ts">
// 试走对比：同一个比较起点、两种选择、各自的对方应对与已确认的差别。
// 评分、优劣方向与差别结论都是程序给出的；搜索不完整或只有边界值时，界面只说明“暂不比较优劣”。
import { computed } from "vue";
import { NButton, NSelect, NSpin, NTag } from "naive-ui";
import type { CompareOption, CompareOutcome, CompareSide } from "../api/types";
import { closeComparison, cmp, demoSide, setReference } from "../state/compare";
import { nodeOf, roundAfter, wb } from "../state/workbench";
import { sideLabel } from "../utils/board";
import CoachAsk from "./CoachAsk.vue";
import Icon from "./Icon.vue";

const view = computed(() => cmp.view);
const comparison = computed(() => view.value?.comparison ?? null);

/** 比较起点：两步都从这个局面走出 */
const originText = computed(() => {
  const o = nodeOf(cmp.request?.origin);
  if (!o) return "";
  const side = sideLabel(o.side_to_move);
  if (!o.mv) return `起始局面 · ${side}先走`;
  return `第 ${roundAfter(o)} 回合 · ${side}走（${o.notation}之后）`;
});

function optionLabel(o: CompareOption): string {
  const rank = o.rank ? `（候选第 ${o.rank}）` : "";
  switch (o.kind) {
    case "actual":
      return `实际着法 ${o.notation}${rank}`;
    case "branch":
      return `棋谱分支 ${o.notation}${rank}`;
    case "candidate":
      return `引擎候选第 ${o.rank} ${o.notation}`;
    default:
      return o.notation;
  }
}

const referenceTag = computed(() => {
  const v = view.value;
  if (!v) return "";
  if (v.reference_kind === "actual") return "实际着法";
  if (v.reference_kind === "branch") return "棋谱分支";
  if (v.reference_kind === "candidate") return `引擎候选第 ${v.reference_rank}`;
  return "其他着法";
});

const selectOptions = computed(() => {
  const v = view.value;
  if (!v) return [];
  const opts = v.options.map((o) => ({ label: optionLabel(o), value: o.mv }));
  if (!opts.some((o) => o.value === v.reference)) opts.unshift({ label: v.comparison.reference.notation, value: v.reference });
  return opts;
});

const OUTCOME: Record<CompareOutcome, { text: string; type: "success" | "warning" | "info" | "default" }> = {
  mine_better: { text: "我的选择更好", type: "success" },
  reference_better: { text: "对照选择更好", type: "warning" },
  close: { text: "两者接近", type: "info" },
  unclear: { text: "暂不比较优劣", type: "default" },
  not_analyzed: { text: "尚未分析", type: "default" },
};
const outcome = computed(() => (comparison.value ? OUTCOME[comparison.value.outcome] : null));

const demoing = computed(() => (wb.preview?.source?.startsWith("compare:") ? wb.preview.source.slice(8) : null));

const sides = computed(() => {
  const c = comparison.value;
  if (!c) return [];
  return [
    { key: "mine" as const, title: "我的选择", tag: null as string | null, side: c.mine },
    { key: "reference" as const, title: "对照选择", tag: referenceTag.value, side: c.reference },
  ];
});

function replyText(s: CompareSide): string {
  if (s.reply) return s.reply.text;
  if (view.value?.pending) return "等待同一次搜索的结果…";
  return "没有可以合法回放的引擎变化";
}
</script>

<template>
  <div class="compare" data-testid="compare-panel">
    <header class="head">
      <div>
        <div class="title">对比两种选择</div>
        <p class="origin" data-testid="compare-origin">比较起点：{{ originText }}，两步都从这里走出</p>
      </div>
      <n-button size="small" quaternary data-testid="compare-close" @click="closeComparison">关闭对比</n-button>
    </header>

    <p v-if="cmp.error" class="error">{{ cmp.error }}</p>
    <div v-else-if="!view" class="loading"><n-spin size="small" /> 正在准备对比…</div>
    <template v-else-if="comparison">
      <label class="ref">
        <span>对照选择</span>
        <n-select
          size="small"
          :value="view.reference"
          :options="selectOptions"
          :consistent-menu-width="false"
          data-testid="compare-reference-select"
          @update:value="setReference"
        />
      </label>

      <!-- 已确认的差别：程序结论 -->
      <section class="diff" data-testid="compare-differences">
        <div class="diff-head">
          <span class="section-name">已确认的差别</span>
          <n-tag v-if="outcome && !(view.pending && comparison.outcome === 'not_analyzed')" size="small" :bordered="false" :type="outcome.type" data-testid="compare-outcome">
            {{ outcome.text }}
          </n-tag>
          <span v-if="view.pending || cmp.loading" class="muted"><n-spin :size="12" /> 正在用同一次搜索比较两步…</span>
        </div>
        <ul v-if="comparison.differences.length" class="points">
          <li v-for="(d, i) in comparison.differences" :key="i">{{ d.text }}</li>
        </ul>
        <p v-else-if="!view.pending" class="muted">两步在规则事实上没有直接可比的差别。</p>
        <p v-if="view.error" class="warn">没有得到比较搜索的结果：{{ view.error }}。下面只列出规则事实。</p>
      </section>

      <!-- 两种选择 -->
      <section v-for="s in sides" :key="s.key" class="side" :class="{ active: demoing === s.key }" :data-testid="`compare-${s.key}`">
        <div class="side-head">
          <n-tag size="small" :bordered="false" round :type="s.key === 'mine' ? 'info' : 'default'">{{ s.title }}</n-tag>
          <strong class="notation">{{ s.side.notation }}</strong>
          <n-tag v-if="s.tag" size="small" :bordered="false">{{ s.tag }}</n-tag>
          <span class="spacer" />
          <!-- 演示使用蓝色：正在演示的一方为实心，另一方为浅色 -->
          <n-button
            size="small"
            type="info"
            :secondary="demoing !== s.key"
            :disabled="!s.side.line"
            :data-testid="`compare-demo-${s.key}`"
            @click="demoSide(s.key)"
          >
            <template #icon><Icon name="play" :size="12" /></template>{{ demoing === s.key ? "正在演示" : "演示" }}
          </n-button>
        </div>
        <p v-if="s.side.score_text" class="score"><span class="score-label">程序</span>{{ s.side.score_text }}</p>
        <h4 class="section-title">这步改变了什么</h4>
        <ul v-if="s.side.changes.length" class="points">
          <li v-for="(c, i) in s.side.changes" :key="i">{{ c.text }}</li>
        </ul>
        <p v-else class="muted">这一步没有可以单独说明的直接作用。</p>
        <h4 class="section-title">对方最值得关注的回应</h4>
        <p class="reply" :class="{ muted: !s.side.reply }">{{ replyText(s.side) }}</p>
      </section>

      <!-- 教练追问：就这组对比提问，回答依据上面的程序结论 -->
      <CoachAsk />

      <p v-if="comparison.conditions" class="meta">分析条件：{{ comparison.conditions }}（两步在同一次搜索中比较，评分为{{ sideLabel(comparison.mover) }}视角）</p>
      <p v-for="w in comparison.warnings" :key="w" class="meta warn">{{ w }}</p>
      <p class="meta">评分、优劣方向与差别结论由程序根据同一次搜索给出；对照选择不一定是最好的走法。</p>
    </template>
  </div>
</template>

<style scoped>
.compare {
  padding: var(--sp-4) var(--sp-4) var(--sp-5);
  font-size: var(--fs-body);
  line-height: var(--lh-body);
}
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sp-2);
}
.title {
  font-family: var(--font-serif);
  font-weight: 700;
  font-size: var(--fs-key);
}
.origin {
  margin: 2px 0 0;
  font-size: var(--fs-group);
  color: var(--text-secondary);
}
.ref {
  display: grid;
  grid-template-columns: auto 1fr;
  align-items: center;
  gap: var(--sp-2);
  margin: var(--sp-3) 0;
  font-size: var(--fs-group);
  color: var(--text-secondary);
}
.diff {
  padding: var(--sp-3) var(--sp-4);
  border-radius: var(--radius-lg);
  background: var(--panel-raised);
  box-shadow: inset 0 0 0 1px var(--border);
}
.diff-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-bottom: var(--sp-1);
}
.section-name {
  font-weight: 600;
  font-size: var(--fs-group);
}
.points {
  list-style: none;
  margin: 0;
  padding: 0;
}
.points li {
  position: relative;
  padding-left: 18px;
}
.points li::before {
  content: "";
  position: absolute;
  left: 5px;
  top: calc(0.85em - 2px);
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--text-muted);
}
.side {
  margin-top: var(--sp-3);
  padding: var(--sp-3) var(--sp-4);
  border-radius: var(--radius-lg);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: box-shadow 0.15s ease;
}
.side.active {
  box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--accent-blue) 60%, transparent);
}
.side-head {
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
.score {
  margin: var(--sp-1) 0 0;
  font-size: var(--fs-basis);
  color: var(--text-secondary);
}
.score-label {
  margin-right: 6px;
  padding: 0 6px;
  border-radius: 4px;
  background: var(--tag-gray-bg);
  font-size: 12px;
  color: var(--text-muted);
}
.side :deep(.section-title) {
  margin: var(--sp-3) 0 var(--sp-1);
}
.reply {
  margin: 0;
}
.muted {
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
.warn {
  color: var(--accent-amber);
}
.error {
  color: var(--accent-red);
}
.meta {
  margin: var(--sp-3) 0 0;
  font-size: var(--fs-meta);
  color: var(--text-muted);
}
.meta.warn {
  color: var(--accent-amber);
}
.loading {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-top: var(--sp-4);
  color: var(--text-muted);
}
</style>
