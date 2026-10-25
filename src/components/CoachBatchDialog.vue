<script setup lang="ts">
// 批量生成 AI 讲解：选择范围后先看要生成几着、大约调用几次、今天还剩几次，确认后逐着生成。
import { computed, watch } from "vue";
import { NAlert, NButton, NModal, NProgress, NRadioButton, NRadioGroup, NTag } from "naive-ui";
import { batch, batchActive, pauseBatch, prepareBatch, resumeBatch, retryFailed, startBatch, stopBatch, type ItemState } from "../state/coachBatch";
import { batchTargets, SCOPE_LABEL, type BatchScope } from "../state/coachBatchPlan";
import { nodeOf, selectNode, shownNotation, study, wb } from "../state/workbench";
import { moveNumber } from "../utils/board";

const SCOPES: BatchScope[] = ["starred", "line", "mainline"];
const STATE: Record<ItemState, { text: string; type: "default" | "info" | "success" | "error" }> = {
  waiting: { text: "未生成", type: "default" },
  running: { text: "生成中", type: "info" },
  done: { text: "已生成", type: "success" },
  failed: { text: "失败", type: "error" },
};

const coach = computed(() => wb.status?.coach ?? null);
// 运行或暂停中不能改范围；暂停后可以继续或停止
const locked = computed(() => batchActive() || batch.phase === "paused");
const counts = computed(() => {
  const s = study.value;
  return Object.fromEntries(SCOPES.map((k) => [k, s ? batchTargets(s, k, wb.selected).length : 0])) as Record<BatchScope, number>;
});
const plan = computed(() => batch.plan);
const done = computed(() => batch.items.filter((i) => i.state === "done").length);
const failed = computed(() => batch.items.filter((i) => i.state === "failed").length);
const waiting = computed(() => batch.items.filter((i) => i.state === "waiting").length);
const percent = computed(() => (batch.items.length ? Math.round(((done.value + failed.value) / batch.items.length) * 100) : 0));
const left = computed(() => (coach.value ? (coach.value.daily_budget === 0 ? null : Math.max(0, coach.value.daily_budget - coach.value.used_today)) : null));

const phaseText = computed(() => {
  switch (batch.phase) {
    case "running":
      return "正在逐着生成…";
    case "pausing":
      return "当前这一着完成后暂停";
    case "paused":
      return "已暂停";
    case "stopping":
      return "正在停止：尚未发出的请求不再发送";
    case "finished":
      return batch.ended ?? (failed.value ? `完成，${failed.value} 着失败，可以重试` : "全部完成");
    default:
      return "";
  }
});

function label(id: number) {
  const node = nodeOf(id);
  const parent = nodeOf(node?.parent);
  if (!node || !parent) return "";
  // 先想后讲中尚未揭示的着法只显示回合与走棋方
  return `${moveNumber(node.ply, nodeOf(0)?.side_to_move ?? "red", parent.side_to_move)} ${shownNotation(id, `${parent.side_to_move === "red" ? "红方" : "黑方"} · 尚未揭示`)}`;
}

// 打开时按当前着法重新计划（运行或暂停中保持原样，显示进度）
watch(
  () => wb.dialogs.coachBatch,
  (open) => {
    if (open && !locked.value) void prepareBatch(batch.scope);
  },
);
</script>

<template>
  <n-modal v-model:show="wb.dialogs.coachBatch" preset="card" title="批量生成 AI 讲解" style="width: min(600px, 96vw)" data-testid="coach-batch">
    <n-alert v-if="coach && !coach.ready" type="warning" :show-icon="false" class="block">AI 讲解当前不可用：{{ coach.reason }}</n-alert>

    <div class="block">
      <div class="label">范围</div>
      <n-radio-group :value="batch.scope" size="small" :disabled="locked" @update:value="(v: BatchScope) => prepareBatch(v)">
        <n-radio-button v-for="s in SCOPES" :key="s" :value="s" :data-testid="`batch-scope-${s}`">
          {{ SCOPE_LABEL[s] }}（{{ counts[s] }} 着）
        </n-radio-button>
      </n-radio-group>
    </div>

    <n-alert v-if="batch.error" type="error" :show-icon="false" class="block">{{ batch.error }}</n-alert>
    <p v-else-if="batch.preparing" class="muted block">正在读取这些着法的讲解状态…</p>
    <div v-else-if="plan" class="block" data-testid="batch-summary">
      <p v-if="!plan.need.length && !plan.skip.length">这个范围内没有着法。</p>
      <template v-else>
        <p>
          共 {{ plan.need.length + plan.skip.length }} 着：已有讲解 {{ plan.skip.length }} 着，直接使用；需要生成
          <b>{{ plan.need.length }}</b> 着。
        </p>
        <p v-if="plan.need.length">
          预计调用模型 {{ plan.calls.min }}{{ plan.calls.max > plan.calls.min ? `–${plan.calls.max}` : "" }} 次（每着一次，输出需要修正时再加一次）。
        </p>
        <p class="muted">今天已调用 {{ coach?.used_today ?? 0 }} 次，{{ left === null ? "不限次数" : `还剩 ${left} 次` }}。</p>
      </template>
      <n-alert v-if="left === 0 && waiting" type="warning" :show-icon="false" class="gap">
        今天的调用次数已经用完，可以明天再生成，或在设置中调高每日调用上限。
      </n-alert>
      <n-alert v-else-if="plan.short && batch.phase !== 'finished'" type="warning" :show-icon="false" class="gap">
        今天剩余的次数可能不够生成全部，次数用完时会自动停止。
      </n-alert>
    </div>

    <div v-if="batch.items.length && batch.phase !== 'idle'" class="block" data-testid="batch-progress">
      <n-progress type="line" :percentage="percent" :show-indicator="false" :height="6" />
      <p class="progress">
        已处理 {{ done + failed }} / {{ batch.items.length }}：已生成 {{ done }}<span v-if="failed">，失败 {{ failed }}</span>
        <span class="phase" data-testid="batch-phase"> · {{ phaseText }}</span>
      </p>
      <ol class="items">
        <li v-for="item in batch.items" :key="item.id" :data-batch-node="item.id" :data-state="item.state">
          <button type="button" class="move focusable" @click="selectNode(item.id)">{{ label(item.id) }}</button>
          <n-tag size="tiny" :type="STATE[item.state].type" :bordered="false">{{ STATE[item.state].text }}</n-tag>
          <span v-if="item.reason" class="reason" :title="item.reason">{{ item.reason }}</span>
        </li>
      </ol>
    </div>

    <template #footer>
      <div class="actions">
        <template v-if="batch.phase === 'running'">
          <n-button size="small" data-testid="batch-pause" @click="pauseBatch">暂停</n-button>
          <n-button size="small" data-testid="batch-stop" @click="stopBatch">停止</n-button>
        </template>
        <template v-else-if="batch.phase === 'pausing' || batch.phase === 'paused'">
          <n-button size="small" type="primary" data-testid="batch-resume" @click="resumeBatch">继续</n-button>
          <n-button size="small" data-testid="batch-stop" @click="stopBatch">停止</n-button>
        </template>
        <n-button v-else-if="batch.phase === 'stopping'" size="small" disabled>正在停止…</n-button>
        <template v-else>
          <n-button
            v-if="batch.phase === 'finished' && failed"
            size="small"
            type="primary"
            data-testid="batch-retry"
            :disabled="!coach?.ready || left === 0"
            @click="retryFailed"
          >
            {{ waiting ? `重试失败的着法并继续（${failed + waiting} 着）` : `重试失败的 ${failed} 着` }}
          </n-button>
          <n-button
            v-else
            size="small"
            type="primary"
            data-testid="batch-start"
            :disabled="!coach?.ready || batch.preparing || !waiting || left === 0"
            @click="startBatch"
          >
            {{ batch.phase === "finished" ? `继续生成其余 ${waiting} 着` : `开始生成 ${waiting} 着` }}
          </n-button>
        </template>
        <n-button size="small" quaternary @click="wb.dialogs.coachBatch = false">{{ batchActive() ? "在后台继续" : "关闭" }}</n-button>
      </div>
    </template>
  </n-modal>
</template>

<style scoped>
.block + .block {
  margin-top: var(--sp-3);
}
.label {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 6px;
}
p {
  margin: 0 0 4px;
  line-height: 1.6;
}
.muted {
  color: var(--text-muted);
}
.gap {
  margin-top: 8px;
}
.progress {
  margin-top: 6px;
  font-size: 13px;
}
.phase {
  color: var(--text-muted);
}
.items {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
  max-height: 240px;
  overflow: auto;
  display: grid;
  gap: 4px;
}
.items li {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  min-width: 0;
}
.move {
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  padding: 0;
  cursor: pointer;
  white-space: nowrap;
}
.move:hover {
  text-decoration: underline;
}
/* 失败原因可能很长（如输出检查的全部问题）：显示两行，完整内容在悬停提示中 */
.reason {
  color: var(--text-muted);
  font-size: 12px;
  min-width: 0;
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow-wrap: anywhere;
}
.actions {
  display: flex;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
}
</style>
