<script setup lang="ts">
// 顶栏：学习会话名称、学习方式（看谱学习 / 先想后讲）与全局操作。
// AI 是否自动生成属于 AI 讲解设置，不在这里切换。
import { computed, ref } from "vue";
import { NButton, NDropdown, NInput, NRadioButton, NRadioGroup, NTag, NTooltip, useMessage } from "naive-ui";
import { errorMessage } from "../api/backend";
import { exportStudy, setMode, study, updateStudyPrefs, wb } from "../state/workbench";
import { layout, ui } from "../state/layout";
import Icon from "./Icon.vue";

const message = useMessage();
const editing = ref(false);
const draft = ref("");

function startEdit() {
  draft.value = study.value?.name ?? "";
  editing.value = true;
}

async function commit() {
  editing.value = false;
  if (draft.value.trim() && draft.value !== study.value?.name) {
    await updateStudyPrefs({ name: draft.value.trim() }).catch((e) => message.error(errorMessage(e)));
  }
}

async function doExport() {
  try {
    await exportStudy();
  } catch (e) {
    message.error(errorMessage(e));
  }
}

const captureLabel = computed(() => {
  switch (wb.capture.state) {
    case "locating":
      return "定位中";
    case "awaiting_start":
    case "awaiting_confirmation":
      return "待确认";
    case "tracking":
      return "记录中";
    default:
      return null;
  }
});

const moreOptions = computed(() => [
  { label: "导出", key: "export", disabled: !study.value },
  { label: captureLabel.value ? `屏幕连接（${captureLabel.value}）` : "屏幕连接", key: "capture" },
]);

function onMore(key: string) {
  if (key === "export") void doExport();
  if (key === "capture") wb.dialogs.capture = true;
}
</script>

<template>
  <header class="top" :class="layout">
    <n-button
      v-if="layout === 'medium' && study"
      size="small"
      :type="ui.treeOpen ? 'primary' : 'default'"
      :secondary="ui.treeOpen"
      data-testid="toggle-tree"
      @click="ui.treeOpen = !ui.treeOpen"
    >
      <template #icon><Icon name="book" /></template>棋谱
    </n-button>
    <div class="brand">
      <span class="seal" aria-hidden="true">弈</span>
      <span v-if="layout !== 'narrow'" class="brand-name">象棋学习助手</span>
    </div>
    <span v-if="study && layout !== 'narrow'" class="sep" aria-hidden="true" />
    <div class="title">
      <n-input v-if="editing" v-model:value="draft" size="small" autofocus style="width: 240px" @blur="commit" @keydown.enter="commit" />
      <n-tooltip v-else-if="study">
        <template #trigger>
          <button type="button" class="name" @click="startEdit">{{ study.name }}</button>
        </template>
        点击修改名称
      </n-tooltip>
    </div>
    <n-radio-group :value="wb.mode" size="small" aria-label="学习方式" class="modes" @update:value="setMode">
      <n-tooltip>
        <template #trigger><n-radio-button value="auto">看谱学习</n-radio-button></template>
        选择着法后直接阅读讲解，适合看谱和理解残局
      </n-tooltip>
      <n-tooltip>
        <template #trigger><n-radio-button value="think">先想后讲</n-radio-button></template>
        先显示落子前的局面，走出自己的选择后再揭示讲解与对照，适合训练
      </n-tooltip>
    </n-radio-group>
    <div class="actions">
      <!-- 全局入口保持中性，最明显的强调留给学习区中正在进行的操作 -->
      <n-button size="small" secondary @click="wb.dialogs.newStudy = true">
        <template #icon><Icon name="plus" /></template>新建 / 打开
      </n-button>
      <template v-if="layout !== 'narrow'">
        <n-button size="small" quaternary :disabled="!study" @click="doExport"><template #icon><Icon name="download" /></template>导出</n-button>
        <n-button size="small" quaternary @click="wb.dialogs.capture = true">
          <template #icon><Icon name="monitor" /></template>屏幕连接
          <n-tag v-if="captureLabel" size="tiny" type="success" :bordered="false" round style="margin-left: 6px">{{ captureLabel }}</n-tag>
        </n-button>
      </template>
      <n-dropdown v-else trigger="click" :options="moreOptions" @select="onMore">
        <n-button size="small" quaternary>更多</n-button>
      </n-dropdown>
      <n-button size="small" quaternary @click="wb.dialogs.settings = true"><template #icon><Icon name="sliders" /></template>设置</n-button>
    </div>
  </header>
</template>

<style scoped>
.top {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: 0 var(--sp-4);
  min-height: 52px;
  background: color-mix(in srgb, var(--panel) 82%, transparent);
  backdrop-filter: saturate(1.2) blur(8px);
  box-shadow: 0 1px 0 var(--border), 0 4px 16px -10px rgba(60, 40, 10, 0.25);
  position: relative;
  z-index: 2;
}
.top.narrow {
  gap: var(--sp-2);
  padding: var(--sp-2);
  flex-wrap: wrap;
}
.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  white-space: nowrap;
}
/* 朱砂印章 */
.seal {
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border-radius: 6px;
  background: var(--seal);
  color: #fff6ee;
  font-family: var(--piece-font);
  font-size: 19px;
  line-height: 1;
  box-shadow: inset 0 0 0 1.5px rgba(255, 255, 255, 0.2), 0 1px 2px rgba(120, 20, 10, 0.3);
}
.brand-name {
  font-family: var(--font-serif);
  font-weight: 700;
  font-size: 16px;
  letter-spacing: 0.12em;
}
.sep {
  width: 1px;
  height: 18px;
  background: var(--border-strong);
}
.title {
  flex: 1;
  min-width: 80px;
}
.name {
  max-width: 100%;
  border: none;
  background: none;
  padding: 3px 6px;
  border-radius: var(--radius-sm);
  font: inherit;
  cursor: text;
  font-size: var(--fs-body);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition: background-color 0.15s ease;
}
.name:hover {
  background: var(--hover-bg);
}
.name:focus-visible {
  outline: 2px solid var(--accent-blue);
}
/* 学习方式：嵌在凹槽里的分段按钮 */
.modes {
  padding: 3px;
  border-radius: 10px;
  background: var(--panel-sunken);
  box-shadow: inset 0 1px 2px rgba(60, 40, 10, 0.08);
}
.modes :deep(.n-radio-group__splitor) {
  display: none;
}
.modes :deep(.n-radio-button) {
  border: none !important;
  border-radius: 7px !important;
  background: transparent;
  height: 26px;
  line-height: 26px;
  padding: 0 14px;
  color: var(--text-muted);
  box-shadow: none !important;
  transition: background-color 0.15s ease, color 0.15s ease;
}
.modes :deep(.n-radio-button .n-radio-button__state-border),
.modes :deep(.n-radio-button .n-radio-button__border) {
  display: none;
}
.modes :deep(.n-radio-button--checked) {
  background: var(--panel);
  color: var(--accent-primary);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(60, 40, 10, 0.12), 0 0 0 1px var(--border) !important;
}
.actions {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
}
</style>
