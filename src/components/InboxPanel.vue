<script setup lang="ts">
// 首页的复盘收件箱：每盘的分析进度（引擎空闲时逐盘分析），分析完成后的关键局面数与出题结果；
// 可以打开棋局或本局报告、移出收件箱（棋局与题目都保留）。出题不自动进行：按钮只为分析完、还没有出过题的棋局出题，
// 并写明新增了几道。列表较长时先显示前几盘。
import { computed, onMounted, ref } from "vue";
import { NButton, NPopconfirm, useMessage } from "naive-ui";
import { errorMessage } from "../api/backend";
import type { InboxItem } from "../api/types";
import { generateInbox, inbox, loadInbox, progressText, readyCount, removeFromInbox } from "../state/inbox";
import { openStudy, wb } from "../state/workbench";
import InboxImportDialog from "./InboxImportDialog.vue";

const message = useMessage();
const importing = ref(false);
const expanded = ref(false);
/** 收起时显示的盘数 */
const FOLDED = 5;
const items = computed(() => inbox.list?.items ?? []);
const shown = computed(() => (expanded.value ? items.value : items.value.slice(0, FOLDED)));
const ready = computed(() => readyCount(inbox.list));
const SIDE = { red: "执红", black: "执黑" } as const;

onMounted(() => void loadInbox());

async function open(i: InboxItem, report = false) {
  try {
    await openStudy(i.study_id);
    if (report) wb.dialogs.report = true;
  } catch (e) {
    message.error(errorMessage(e));
  }
}

async function generate() {
  if (await generateInbox()) message.success(inbox.generated ?? "已出题");
}
</script>

<template>
  <section class="inbox surface" data-testid="home-inbox">
    <header class="head">
      <div>
        <h2>复盘收件箱</h2>
        <p class="muted" data-testid="inbox-summary">
          {{ items.length ? inbox.list?.text : "一次导入多盘自己下的棋，程序在引擎空闲时逐盘分析，分析完成后可以一次出题。" }}
        </p>
      </div>
      <div class="actions">
        <n-button size="small" secondary data-testid="inbox-open-import" @click="importing = true">导入棋局</n-button>
        <n-button
          size="small"
          :type="ready ? 'primary' : 'default'"
          :disabled="!ready || inbox.generating"
          :loading="inbox.generating"
          data-testid="inbox-generate"
          @click="generate"
        >
          {{ ready ? `为分析完的 ${ready} 盘出题` : "为分析完的棋局出题" }}
        </n-button>
      </div>
    </header>
    <p v-if="inbox.generated" class="generated" data-testid="inbox-generated">{{ inbox.generated }}</p>
    <p v-if="inbox.error" class="error">{{ inbox.error }}</p>

    <ul v-if="items.length" class="items">
      <li v-for="i in shown" :key="i.study_id" class="item" :data-inbox-item="i.study_id">
        <div class="info">
          <div class="name">
            {{ i.name }}
            <span class="muted">· {{ SIDE[i.user_side] }}<template v-if="i.date"> · {{ i.date }}</template><template v-if="i.result"> · {{ i.result }}</template></span>
          </div>
          <div class="muted sub" :class="{ running: !i.done }" data-testid="inbox-progress">{{ progressText(i) }}</div>
          <div v-if="i.exercises" class="muted sub" data-testid="inbox-exercises">出题结果：{{ i.exercises.text }}</div>
        </div>
        <div class="item-actions">
          <n-button size="tiny" quaternary @click="open(i)">打开棋局</n-button>
          <n-button size="tiny" quaternary :disabled="!i.done" data-testid="inbox-report" @click="open(i, true)">本局报告</n-button>
          <n-popconfirm @positive-click="removeFromInbox(i.study_id)">
            <template #trigger><n-button size="tiny" quaternary data-testid="inbox-remove">移出</n-button></template>
            从收件箱移出这盘棋？棋局和已经出的题都保留。
          </n-popconfirm>
        </div>
      </li>
    </ul>
    <n-button v-if="items.length > FOLDED" text size="small" class="more" @click="expanded = !expanded">
      {{ expanded ? "收起" : `显示全部 ${items.length} 盘` }}
    </n-button>
    <InboxImportDialog v-model:show="importing" />
  </section>
</template>

<style scoped>
.inbox {
  max-width: 960px;
  margin: var(--sp-3) auto 0;
  padding: var(--sp-3) var(--sp-4);
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  flex-wrap: wrap;
  gap: var(--sp-2);
}
.head h2 {
  margin: 0 0 4px;
  font-family: var(--font-serif);
  font-size: 16px;
}
.head p {
  margin: 0;
  line-height: 1.6;
}
.actions {
  display: flex;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.generated {
  margin: var(--sp-2) 0 0;
  line-height: 1.6;
}
.items {
  list-style: none;
  margin: var(--sp-2) 0 0;
  padding: 0;
  display: grid;
  gap: 4px;
}
.item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: var(--sp-2);
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
}
.info {
  min-width: 0;
}
.name {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sub {
  font-size: 12px;
}
.sub.running {
  color: var(--accent-blue);
}
.item-actions {
  display: flex;
  flex-shrink: 0;
}
.more {
  margin-top: 6px;
}
.muted {
  color: var(--text-muted);
}
.error {
  color: var(--accent-red);
}
</style>
