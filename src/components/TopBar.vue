<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NInput, NRadioButton, NRadioGroup, NTag, NTooltip, useMessage } from "naive-ui";
import { errorMessage } from "../api/backend";
import { exportStudy, setMode, study, updateStudyPrefs, wb } from "../state/workbench";

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
</script>

<template>
  <header class="top">
    <div class="brand">象棋学习助手</div>
    <div class="title">
      <n-input v-if="editing" v-model:value="draft" size="small" autofocus style="width: 240px" @blur="commit" @keydown.enter="commit" />
      <n-tooltip v-else-if="study">
        <template #trigger>
          <span class="name" @click="startEdit">{{ study.name }}</span>
        </template>
        点击修改名称
      </n-tooltip>
    </div>
    <n-radio-group :value="wb.mode" size="small" @update:value="setMode">
      <n-tooltip>
        <template #trigger><n-radio-button value="auto">自动讲解</n-radio-button></template>
        每着落子后直接显示讲解，适合看谱和理解残局
      </n-tooltip>
      <n-tooltip>
        <template #trigger><n-radio-button value="think">先想后讲</n-radio-button></template>
        先在棋盘上走出自己的选择，再显示讲解和对照，适合训练
      </n-tooltip>
    </n-radio-group>
    <div class="actions">
      <n-button size="small" @click="wb.dialogs.newStudy = true">新建 / 打开</n-button>
      <n-button size="small" :disabled="!study" @click="doExport">导出</n-button>
      <n-button size="small" @click="wb.dialogs.capture = true">
        屏幕连接
        <n-tag v-if="captureLabel" size="tiny" type="success" :bordered="false" style="margin-left: 6px">{{ captureLabel }}</n-tag>
      </n-button>
      <n-button size="small" quaternary @click="wb.dialogs.settings = true">设置</n-button>
    </div>
  </header>
</template>

<style scoped>
.top {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
}
.brand {
  font-weight: 700;
  letter-spacing: 1px;
}
.title {
  flex: 1;
  min-width: 0;
}
.name {
  cursor: text;
  font-size: 15px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.actions {
  display: flex;
  gap: 8px;
}
</style>
