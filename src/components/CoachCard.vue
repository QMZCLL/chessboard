<script setup lang="ts">
// AI 讲解：由语言模型根据规则事实与引擎证据组织文字。
// 显示方式见 state/coachDisplay.ts：
// - 结果对应当前条件：作为“为什么这样走”的正文，各段下方紧邻程序给出的依据原文（评分、排名、评价与杀棋结论只在依据中）。
//   一句话摘要由程序的要点承担，学习要点放在讲解面板的学习点中，这里不重复显示；
// - 只剩上次的结果：正文仍是规则讲解，上次结果折叠显示并注明，展开后显示完整的上次讲解；
// - 没有结果：只占一行，说明当前状态与下一步可以做什么。状态全部来自后台，不在界面另做判断。
import { computed, ref, watch } from "vue";
import { NButton, NSpin, NTag } from "naive-ui";
import type { CoachResult, ExplainLevel } from "../api/types";
import { coachLesson, coachSections, paragraphOf } from "../state/coachContent";
import { coachDisplay } from "../state/coachDisplay";
import { canDemo, cancelCoach, coachView, demoEvidence, openSettings, requestCoach, wb } from "../state/workbench";
import BasisList from "./BasisList.vue";
import ClaimList from "./ClaimList.vue";
import Icon from "./Icon.vue";

const props = defineProps<{ level: ExplainLevel | null; pending: boolean }>();

const view = coachView;
const result = computed<CoachResult | null>(() => view.value?.record?.result ?? null);
const status = computed(() => view.value?.status ?? null);
const display = computed(() => coachDisplay(view.value));
const busy = computed(() => status.value === "queued" || status.value === "running");
const showDetail = ref(false);
/** 上次的讲解默认折叠 */
const expanded = ref(false);

watch(
  () => [view.value?.study_id, view.value?.node_id],
  () => {
    showDetail.value = false;
    expanded.value = false;
  },
);

const AUDIENCE: Record<string, string> = { beginner: "入门", intermediate: "进阶", advanced: "高级" };
const DETAIL: Record<string, string> = { brief: "简短", normal: "常规篇幅", deep: "详细" };

const meta = computed(() => {
  const c = wb.status?.config.coach;
  return c ? `${AUDIENCE[c.audience]} · ${DETAIL[c.detail]}` : "";
});

/** 分析状态与 AI 讲解状态分开说明。 */
const analysisText = computed(() => {
  if (props.pending) return "引擎分析进行中";
  switch (props.level) {
    case "detailed":
      return "引擎分析完整";
    case "partial":
      return "引擎证据不完整，AI 只解释已确认的内容";
    case "basic":
      return "只有规则讲解，AI 只解释规则事实";
    default:
      return "";
  }
});

/** 只有上次的结果时，说明正文用的是什么、上次的结果为什么不再对应。 */
const previousNote = computed(() => {
  if (status.value === "failed") return "更新失败，正文使用当前的规则讲解；上次的 AI 讲解可能与现在的分析不一致。";
  if (busy.value) return "正在按当前的分析与设置更新；完成前正文使用规则讲解。";
  return "上次的 AI 讲解与现在的分析或设置不再对应，正文使用当前的规则讲解。";
});

/** 失败或过期原因的简短说明：取第一句，详情可展开。 */
const shortReason = computed(() => {
  const r = view.value?.reason ?? "";
  const first = r.split(/[；;]/)[0];
  return first.length > 48 ? `${first.slice(0, 48)}…` : first;
});

const sections = computed(() => (result.value ? coachSections(result.value) : []));
const summary = computed(() => (result.value ? paragraphOf(result.value, result.value.output.summary) : null));
const lesson = computed(() => coachLesson(result.value));

const evidenceById = (id: string | null) => result.value?.evidence.find((e) => e.id === id);
/** 后续操作只保留有可演示变化的。 */
const followups = computed(() => (result.value?.output.followups ?? []).filter((f) => canDemo(evidenceById(f.evidence_id))));

function follow(id: string | null) {
  const ev = evidenceById(id);
  if (ev) demoEvidence(ev);
}
</script>

<template>
  <!-- 有结果：对应当前条件时作为正文；只剩上次的结果时折叠显示并注明 -->
  <section
    v-if="(display.kind === 'current' || display.kind === 'previous') && result"
    class="coach body"
    :class="display.kind"
    data-testid="coach-card"
  >
    <header>
      <span class="title"><Icon name="sparkles" :size="15" /> {{ display.kind === "previous" ? "上次的 AI 讲解" : "AI 讲解" }}</span>
      <n-tag size="small" :bordered="false" :type="display.badge.tone" data-testid="coach-status">
        <n-spin v-if="display.busy" :size="10" /> {{ display.badge.text }}
      </n-tag>
      <span class="spacer" />
      <n-button v-if="busy" size="small" quaternary @click="cancelCoach">取消</n-button>
      <n-button v-else-if="display.kind === 'previous'" size="small" type="primary" secondary @click="requestCoach(status === 'failed')">
        {{ status === "failed" ? "重试" : "更新" }}
      </n-button>
      <n-button v-else size="small" quaternary @click="requestCoach(true)">重新生成</n-button>
    </header>
    <p v-if="display.kind === 'previous'" class="note" data-testid="coach-previous-note">{{ previousNote }}</p>
    <p v-if="view?.reason && !busy" class="note warn">
      {{ shortReason }}
      <n-button v-if="shortReason !== view.reason" text size="tiny" @click="showDetail = !showDetail">{{ showDetail ? "收起" : "详情" }}</n-button>
    </p>
    <p v-if="showDetail" class="note">{{ view?.reason }}</p>
    <n-button v-if="display.kind === 'previous'" text size="small" type="info" data-testid="coach-previous-toggle" @click="expanded = !expanded">
      {{ expanded ? "收起上次的讲解" : "查看上次的讲解" }}
    </n-button>

    <template v-if="display.kind === 'current' || expanded">
      <div :class="{ faded: display.kind === 'previous' }">
        <!-- 上次的讲解作为参考完整显示；当前有效的讲解中，摘要与学习要点由讲解面板的要点与学习点承担 -->
        <template v-if="display.kind === 'previous' && summary">
          <ClaimList scope="ai-summary" :claims="[summary.claim]" :evidence="result.evidence" class="lead" />
          <BasisList :items="summary.basis" />
        </template>
        <template v-for="(s, i) in sections" :key="i">
          <h5 v-if="s.title">{{ s.title }}</h5>
          <ClaimList :scope="`ai-${i}`" :claims="[s.claim]" :evidence="result.evidence" />
          <BasisList :items="s.basis" />
        </template>
        <template v-if="display.kind === 'previous' && lesson">
          <h5>学习要点</h5>
          <ClaimList scope="ai-lesson" :claims="[lesson.claim]" :evidence="result.evidence" />
          <BasisList :items="lesson.basis" />
        </template>
        <div v-if="followups.length" class="followups">
          <n-button v-for="f in followups" :key="f.action" size="small" secondary @click="follow(f.evidence_id)">
            <template #icon><Icon name="play" :size="12" /></template>{{ f.label }}
          </n-button>
        </div>
      </div>
      <p class="source">
        {{ analysisText }}。由 {{ result.model_label }} 根据程序给出的事实与引擎证据组织文字；评分、排名、评价与杀棋结论只出现在程序给出的“依据”中。
      </p>
    </template>
  </section>

  <!-- 没有结果：一行说明当前状态 -->
  <section v-else-if="display.kind === 'entry' && view" class="coach entry" data-testid="coach-card">
    <template v-if="status === 'disabled'">
      <span class="muted" data-testid="coach-status">可开启 AI 讲解，帮助理解本步理由</span>
      <n-button size="small" quaternary @click="openSettings('coach')">开启 AI 讲解</n-button>
    </template>
    <template v-else-if="status === 'none'">
      <n-button size="small" type="primary" secondary data-testid="coach-generate" @click="requestCoach(false)">
        <template #icon><Icon name="sparkles" /></template>生成 AI 讲解
      </n-button>
      <span class="muted" data-testid="coach-status">{{ meta }}<template v-if="view.reason"> · {{ view.reason }}</template></span>
    </template>
    <template v-else-if="busy">
      <n-spin :size="12" />
      <span class="muted" data-testid="coach-status">{{ status === "queued" ? view.reason ?? "AI 讲解等待生成…" : "AI 讲解生成中…" }}</span>
      <n-button size="small" quaternary @click="cancelCoach">取消</n-button>
    </template>
    <template v-else-if="status === 'failed'">
      <span class="warn" data-testid="coach-status">AI 讲解生成失败：{{ shortReason }}</span>
      <n-button size="small" quaternary @click="requestCoach(true)">重试</n-button>
      <n-button v-if="shortReason !== view.reason" text size="tiny" @click="showDetail = !showDetail">{{ showDetail ? "收起" : "详情" }}</n-button>
      <p v-if="showDetail" class="note full">{{ view.reason }}</p>
    </template>
  </section>
</template>

<style scoped>
/* 当前有效的 AI 讲解是正文：不加卡片外框，标题行与面板中的其他分组一致 */
.coach.body.current {
  margin: var(--sp-4) 0 var(--sp-2);
}
.coach.body.current header {
  padding-bottom: 6px;
  margin-bottom: var(--sp-2);
  border-bottom: 1px solid var(--border);
}
/* 上次的讲解：作为参考的次要卡片 */
.coach.body.previous {
  margin: var(--sp-3) 0;
  padding: var(--sp-3) var(--sp-4) var(--sp-4);
  border-radius: var(--radius-lg);
  background: var(--panel);
  box-shadow: inset 0 0 0 1px var(--border-strong);
}
.coach.entry {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin: var(--sp-3) 0;
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--radius);
  background: var(--panel-raised);
  box-shadow: inset 0 0 0 1px var(--border);
  font-size: var(--fs-aux);
}
header {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
  margin-bottom: var(--sp-1);
}
.title {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-weight: 600;
  font-size: var(--fs-group);
  color: var(--accent-primary);
}
.coach.body.previous .title {
  color: var(--text-secondary);
}
.spacer {
  flex: 1;
}
.muted {
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
.warn {
  color: var(--accent-amber);
}
.note {
  margin: var(--sp-1) 0;
  font-size: var(--fs-aux);
  color: var(--text-secondary);
}
.note.warn {
  color: var(--accent-amber);
}
.note.full {
  flex-basis: 100%;
}
.lead :deep(.claim) {
  font-weight: 600;
}
h5 {
  margin: var(--sp-3) 0 2px;
  font-size: var(--fs-group);
  color: var(--text-secondary);
  font-weight: 600;
}
.faded {
  opacity: 0.7;
}
.followups {
  display: flex;
  gap: var(--sp-2);
  flex-wrap: wrap;
  margin-top: var(--sp-3);
}
.source {
  margin: var(--sp-3) 0 0;
  font-size: var(--fs-meta);
  color: var(--text-muted);
}
</style>
