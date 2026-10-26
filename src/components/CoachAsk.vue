<script setup lang="ts">
// 教练追问：就当前这组对比提出一个结构化的问题。回答的文字由模型组织，每段下方是它引用的程序结论原文；
// 演示的变化由程序按问题选择。评分、优劣与杀棋结论只出现在程序结论中。
import { computed } from "vue";
import { NButton, NSpin, NTooltip } from "naive-ui";
import type { AnswerView } from "../api/types";
import { ask, askCoach, demoAnswer, questions, type QuestionItem } from "../state/coachAsk";
import { basisOf, questionKey } from "../state/coachQuestions";
import { budgetLeft } from "../state/coachBatchPlan";
import { openSettings, wb } from "../state/workbench";
import BasisList from "./BasisList.vue";
import Icon from "./Icon.vue";

const coach = computed(() => wb.status?.coach ?? null);
/** AI 讲解不可用的原因（未启用、没有密钥等） */
const unavailable = computed(() => (coach.value && !coach.value.ready ? (coach.value.reason ?? "AI 讲解当前不可用") : null));

const budgetText = computed(() => {
  const c = coach.value;
  if (!c?.ready) return "";
  const left = budgetLeft(c);
  const per = `每个问题通常调用模型 ${c.calls_per_move.usual} 次，输出需要修正时 ${c.calls_per_move.with_repair} 次`;
  return left === null ? `${per}，计入今天的调用次数` : `${per}；今天还剩 ${left} 次`;
});

/** 已经提过的问题（按问题顺序）：回答、进度、失败或过期后的重新提问都在各自的卡片中 */
const isAsked = (q: QuestionItem) => !!(q.view?.record || q.error || q.sending);
const asked = computed(() => questions.value.filter(isAsked));
/** 还没有提过的问题 */
const open = computed(() => questions.value.filter((q) => !isAsked(q)));

const demoing = computed(() => wb.preview?.source ?? null);
const demoKey = (v: AnswerView, i: number) => `ask:${questionKey(v.question)}:${i}`;

function statusText(q: QuestionItem): string | null {
  const v = q.view;
  if (q.sending || v?.status === "queued") return "已提问，等待回答…";
  if (v?.status === "running") return "正在回答…";
  return null;
}
</script>

<template>
  <section class="ask" data-testid="coach-ask">
    <div class="ask-head">
      <span class="section-name"><Icon name="sparkles" :size="14" /> 问问教练</span>
      <span v-if="budgetText" class="muted" data-testid="coach-ask-budget">{{ budgetText }}</span>
    </div>
    <p v-if="unavailable" class="muted" data-testid="coach-ask-unavailable">
      {{ unavailable }}
      <n-button text type="primary" size="small" @click="openSettings('coach')">去设置</n-button>
    </p>
    <div v-if="open.length" class="questions">
      <n-tooltip v-for="q in open" :key="q.kind" :disabled="!q.blocked && !unavailable">
        <template #trigger>
          <!-- 禁用的按钮不响应鼠标，外面包一层以显示原因 -->
          <span>
            <n-button
              size="small"
              secondary
              :disabled="!!q.blocked || !!unavailable"
              :loading="q.sending"
              :data-testid="`coach-ask-${q.kind}`"
              @click="askCoach(q.kind)"
            >
              {{ q.label }}
            </n-button>
          </span>
        </template>
        {{ unavailable ?? q.blocked }}
      </n-tooltip>
    </div>
    <p v-if="ask.loadError" class="error">{{ ask.loadError }}</p>

    <article v-for="q in asked" :key="q.key" class="answer" :data-testid="`coach-answer-${q.kind}`">
      <h4 class="question">{{ q.label }}</h4>
      <p v-if="statusText(q)" class="muted"><n-spin :size="12" /> {{ statusText(q) }}</p>
      <p v-if="q.error" class="error" data-testid="coach-answer-error">
        {{ q.error }}
        <n-button text type="primary" size="small" :disabled="!!unavailable || !!q.blocked" @click="askCoach(q.kind)">重新提问</n-button>
      </p>
      <p v-if="q.view?.status === 'failed'" class="error" data-testid="coach-answer-error">
        没有得到回答：{{ q.view.reason }}
        <n-button text type="primary" size="small" :disabled="!!unavailable || !!q.blocked" @click="askCoach(q.kind)">重新提问</n-button>
      </p>
      <template v-if="q.view?.record?.result">
        <p v-if="q.view.status === 'stale'" class="warn" data-testid="coach-answer-stale">
          {{ q.view.reason ?? "依据或设置已改变" }}
          <n-button text type="primary" size="small" :disabled="!!unavailable || !!q.blocked" @click="askCoach(q.kind)">重新回答</n-button>
        </p>
        <div :class="{ faded: !q.view.result_current }">
          <p class="text" data-testid="coach-answer-text">{{ q.view.record.result.output.answer.text }}</p>
          <BasisList :items="basisOf(q.view.record.result.output.answer, q.view.record.result.claims)" />
          <template v-for="(p, i) in q.view.record.result.output.points" :key="i">
            <p class="text">{{ p.text }}</p>
            <BasisList :items="basisOf(p, q.view.record.result.claims)" />
          </template>
          <div class="demos">
            <n-button
              v-for="(d, i) in q.view.record.result.demos"
              :key="i"
              size="small"
              type="info"
              :secondary="demoing !== demoKey(q.view, i)"
              :data-testid="`coach-answer-demo-${q.kind}`"
              @click="demoAnswer(q.view, i)"
            >
              <template #icon><Icon name="play" :size="12" /></template>{{ d.label }}
            </n-button>
          </div>
          <p class="check" data-testid="coach-answer-check"><span class="check-label">想一想</span>{{ q.view.record.result.output.check }}</p>
        </div>
        <p class="meta">
          由 {{ q.view.record.result.model_label }} 根据这组对比中程序给出的结论组织文字<template v-if="!q.view.record.result.conditions">（只有规则事实，没有搜索结果）</template>；评分、优劣与杀棋结论只出现在“依据”中。
        </p>
      </template>
    </article>
  </section>
</template>

<style scoped>
.ask {
  margin-top: var(--sp-3);
  padding: var(--sp-3) var(--sp-4);
  border-radius: var(--radius-lg);
  box-shadow: inset 0 0 0 1px var(--border);
}
.ask-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
}
.section-name {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-weight: 600;
  font-size: var(--fs-group);
}
.questions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-top: var(--sp-2);
}
.questions :deep(.n-button) {
  max-width: 100%;
  white-space: normal;
  height: auto;
  min-height: 28px;
  text-align: left;
}
.answer {
  margin-top: var(--sp-3);
  padding-top: var(--sp-3);
  border-top: 1px solid var(--border);
}
.question {
  margin: 0 0 var(--sp-1);
  font-size: var(--fs-group);
}
.text {
  margin: var(--sp-1) 0 0;
}
.answer :deep(.basis) {
  margin-left: 0;
}
.demos {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-top: var(--sp-2);
}
.check {
  margin: var(--sp-2) 0 0;
}
.check-label {
  margin-right: 6px;
  padding: 0 6px;
  border-radius: 4px;
  background: var(--tag-gray-bg);
  font-size: 12px;
  color: var(--text-muted);
}
.faded {
  opacity: 0.7;
}
.muted {
  margin: var(--sp-1) 0 0;
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
.warn {
  margin: var(--sp-1) 0 0;
  color: var(--accent-amber);
}
.error {
  margin: var(--sp-1) 0 0;
  color: var(--accent-red);
}
.meta {
  margin: var(--sp-2) 0 0;
  font-size: var(--fs-meta);
  color: var(--text-muted);
}
</style>
