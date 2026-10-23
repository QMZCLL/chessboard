<script setup lang="ts">
// AI 讲解：由语言模型根据规则事实与引擎证据组织文字。每段都引用程序给出的结论，
// 点击可在棋盘上高亮或回放；评分等数字、排名、评价等级、优劣与杀棋结论不由模型书写，只出现在每段下方由程序给出的依据中。
import { computed } from "vue";
import { NButton, NSpin, NTag } from "naive-ui";
import type { Claim, CoachResult, CoachText, ExplainLevel } from "../api/types";
import { cancelCoach, coachView, requestCoach, showEvidence, wb } from "../state/workbench";
import ClaimList from "./ClaimList.vue";

const props = defineProps<{ level: ExplainLevel | null; pending: boolean }>();

const view = coachView;
const result = computed<CoachResult | null>(() => view.value?.record?.result ?? null);
const busy = computed(() => view.value?.status === "queued" || view.value?.status === "running");

const SECTION: Record<string, string> = {
  purpose: "为什么考虑这步",
  reason: "为什么这样走",
  comparison: "换一种走法",
  reply: "下一步注意",
  caution: "风险提示",
};

const STATUS: Record<string, { text: string; type: "default" | "info" | "success" | "warning" | "error" }> = {
  disabled: { text: "未启用", type: "default" },
  none: { text: "尚未生成", type: "default" },
  queued: { text: "排队中", type: "info" },
  running: { text: "生成中", type: "info" },
  ready: { text: "已生成", type: "success" },
  failed: { text: "生成失败", type: "error" },
  stale: { text: "已过期", type: "warning" },
};

/** 分析状态与 AI 讲解状态分开说明。 */
const analysisText = computed(() => {
  if (props.pending) return "引擎分析进行中";
  switch (props.level) {
    case "detailed":
      return "引擎分析完整";
    case "partial":
      return "引擎证据不完整 · AI 只解释已确认的内容";
    case "basic":
      return "只有规则讲解 · AI 只解释规则事实";
    default:
      return "";
  }
});

/** 一段文字的依据：所引用结论的证据。 */
function asClaim(t: CoachText): Claim {
  const claims = result.value?.claims ?? [];
  const evidence = t.claim_ids.flatMap((id) => claims.find((c) => c.id === id)?.evidence_ids ?? []);
  return { text: t.text, evidence: [...new Set(evidence)] };
}

/** 段落引用的程序结论原文（含评分等数字）。 */
function basisOf(t: CoachText): string[] {
  const claims = result.value?.claims ?? [];
  return t.claim_ids.map((id) => claims.find((c) => c.id === id)?.statement).filter((s): s is string => !!s);
}

const sections = computed(() =>
  (result.value?.output.sections ?? []).map((s) => ({ title: SECTION[s.kind] ?? "", claim: asClaim(s), basis: basisOf(s) })),
);

function follow(id: string | null) {
  const ev = result.value?.evidence.find((e) => e.id === id);
  if (ev) showEvidence(ev);
}
</script>

<template>
  <section v-if="view" class="coach" data-testid="coach-card">
    <header>
      <span class="title">AI 讲解</span>
      <n-tag size="small" :bordered="false" :type="STATUS[view.status].type" data-testid="coach-status">{{ STATUS[view.status].text }}</n-tag>
      <span class="muted">{{ analysisText }}</span>
      <span class="spacer" />
      <template v-if="view.status !== 'disabled'">
        <n-button v-if="busy" size="tiny" quaternary @click="cancelCoach">取消</n-button>
        <n-button v-else-if="!result" size="tiny" type="primary" secondary @click="requestCoach(false)">生成 AI 讲解</n-button>
        <n-button v-else size="tiny" quaternary @click="requestCoach(view.status !== 'stale')">重新生成</n-button>
      </template>
      <n-button v-else size="tiny" quaternary @click="wb.dialogs.settings = true">配置 AI 讲解</n-button>
    </header>

    <div v-if="busy" class="note"><n-spin :size="12" /> {{ view.reason ?? "正在根据上面的证据组织讲解…" }}</div>
    <p v-else-if="view.reason" class="note" :class="{ warn: view.status === 'failed' || view.status === 'stale' }">{{ view.reason }}</p>

    <template v-if="result">
      <div :class="{ faded: view.status === 'stale' }">
        <ClaimList :claims="[asClaim(result.output.summary)]" :evidence="result.evidence" class="lead" />
        <ul v-if="basisOf(result.output.summary).length" class="basis" data-testid="coach-basis">
          <li v-for="(b, j) in basisOf(result.output.summary)" :key="j">依据：{{ b }}</li>
        </ul>
        <template v-for="(s, i) in sections" :key="i">
          <h5 v-if="s.title">{{ s.title }}</h5>
          <ClaimList :claims="[s.claim]" :evidence="result.evidence" />
          <ul v-if="s.basis.length" class="basis" data-testid="coach-basis">
            <li v-for="(b, j) in s.basis" :key="j">依据：{{ b }}</li>
          </ul>
        </template>
        <template v-if="result.output.lesson.text">
          <h5>学习要点</h5>
          <ClaimList :claims="[asClaim(result.output.lesson)]" :evidence="result.evidence" />
          <ul v-if="basisOf(result.output.lesson).length" class="basis" data-testid="coach-basis">
            <li v-for="(b, j) in basisOf(result.output.lesson)" :key="j">依据：{{ b }}</li>
          </ul>
        </template>
        <div v-if="result.output.followups.length" class="followups">
          <n-button v-for="f in result.output.followups" :key="f.action" size="tiny" secondary @click="follow(f.evidence_id)">
            ▶ {{ f.label }}
          </n-button>
        </div>
      </div>
      <p class="source">
        由 {{ result.model_label }} 根据程序给出的事实与引擎证据组织文字，点击段落查看依据；评分、排名、评价与杀棋结论只出现在程序给出的“依据”中。
      </p>
    </template>
  </section>
</template>

<style scoped>
.coach {
  margin: 10px 0 4px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
}
header {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.title {
  font-weight: 600;
  font-size: 14px;
}
.spacer {
  flex: 1;
}
.muted {
  color: var(--text-muted);
  font-size: 12px;
}
.note {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 6px 0 0;
  font-size: 13px;
  color: var(--text-muted);
}
.note.warn {
  color: var(--accent-amber);
}
.lead :deep(li) {
  font-weight: 600;
}
h5 {
  margin: 8px 0 2px;
  font-size: 12px;
  color: var(--text-muted);
  font-weight: 600;
}
.basis {
  list-style: none;
  margin: 0 0 2px;
  padding: 0 8px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-muted);
}
.faded {
  opacity: 0.65;
}
.followups {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  margin-top: 8px;
}
.source {
  margin: 8px 0 0;
  font-size: 11px;
  color: var(--text-muted);
}
</style>
