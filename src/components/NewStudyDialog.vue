<script setup lang="ts">
// 建立学习局面：标准开局、手动摆棋、FEN、棋谱导入，或打开已保存的学习会话。
import { computed, ref, watch } from "vue";
import { NButton, NEmpty, NInput, NModal, NPopconfirm, NRadioButton, NRadioGroup, NTabPane, NTabs, NTag, useMessage } from "naive-ui";
import BoardEditor from "./BoardEditor.vue";
import { api, errorMessage, isTauri } from "../api/backend";
import type { Color, Orientation, StudySummary } from "../api/types";
import { createStudy, importFromFile, importText, openStudy, placementForEditor, study, wb } from "../state/workbench";
import { EMPTY_KINGS_BOARD, sideOfFen } from "../utils/board";

const message = useMessage();
const tab = ref<"standard" | "setup" | "fen" | "record" | "saved">("standard");
const busy = ref(false);
const name = ref("");

const userSide = ref<Color>("red");
const setupBoard = ref(EMPTY_KINGS_BOARD);
const setupSide = ref<Color | null>(null);
const setupOrientation = ref<Orientation>("red_bottom");
const setupValid = ref(false);

const fenText = ref("");
const fenSide = ref<Color | null>(null);
const fenHasSide = computed(() => sideOfFen(fenText.value) !== null);

const recordText = ref("");
const saved = ref<StudySummary[]>([]);

const SOURCE: Record<string, string> = { standard: "标准开局", setup: "摆棋", fen: "FEN", record: "棋谱", capture: "屏幕连接" };

async function loadSaved() {
  try {
    saved.value = await api.listStudies();
  } catch (e) {
    message.error(errorMessage(e));
  }
}

watch(
  () => wb.dialogs.newStudy,
  (open) => {
    if (open) {
      name.value = "";
      void loadSaved();
      if (study.value) setupBoard.value = placementForEditor() || EMPTY_KINGS_BOARD;
    }
  },
);
watch(tab, (t) => t === "saved" && loadSaved());

async function run(task: () => Promise<unknown>) {
  busy.value = true;
  try {
    await task();
    wb.dialogs.newStudy = false;
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    busy.value = false;
  }
}

function startStandard() {
  return run(() =>
    createStudy({
      name: name.value || null,
      kind: { type: "standard" },
      orientation: userSide.value === "red" ? "red_bottom" : "black_bottom",
      user_side: userSide.value,
    }),
  );
}

function startSetup() {
  if (!setupSide.value) return;
  return run(() =>
    createStudy({
      name: name.value || null,
      kind: { type: "setup", board: setupBoard.value, side: setupSide.value! },
      orientation: setupOrientation.value,
      user_side: setupOrientation.value === "red_bottom" ? "red" : "black",
    }),
  );
}

function startFen() {
  return run(() =>
    createStudy({
      name: name.value || null,
      kind: { type: "fen", fen: fenText.value.trim(), side: fenHasSide.value ? null : fenSide.value },
      orientation: null,
      user_side: null,
    }),
  );
}

function startRecord() {
  return run(() => importText(recordText.value, name.value || null));
}

async function readFile(ev: Event) {
  const file = (ev.target as HTMLInputElement).files?.[0];
  if (file) recordText.value = await file.text();
}

async function remove(id: string) {
  try {
    await api.deleteStudy(id);
    await loadSaved();
  } catch (e) {
    message.error(errorMessage(e));
  }
}
</script>

<template>
  <n-modal
    v-model:show="wb.dialogs.newStudy"
    preset="card"
    title="建立学习局面"
    style="width: min(980px, 96vw)"
    :mask-closable="!!study"
    :closable="!!study"
  >
    <n-tabs v-model:value="tab" type="segment" size="small">
      <n-tab-pane name="standard" tab="标准开局">
        <div class="pane">
          <p class="muted">从开局开始逐着学习。你执哪一方，棋盘就以哪一方在下。</p>
          <n-radio-group v-model:value="userSide" size="small">
            <n-radio-button value="red">我执红</n-radio-button>
            <n-radio-button value="black">我执黑</n-radio-button>
          </n-radio-group>
          <n-input v-model:value="name" placeholder="名称（可选）" size="small" />
          <n-button type="primary" :loading="busy" @click="startStandard">开始</n-button>
        </div>
      </n-tab-pane>

      <n-tab-pane name="setup" tab="摆棋 / 残局">
        <BoardEditor
          v-model:board="setupBoard"
          v-model:side="setupSide"
          v-model:orientation="setupOrientation"
          show-orientation
          @validity="(ok: boolean) => (setupValid = ok)"
        />
        <div class="footer">
          <n-input v-model:value="name" placeholder="名称（可选）" size="small" style="max-width: 280px" />
          <n-button type="primary" :loading="busy" :disabled="!setupValid || !setupSide" @click="startSetup">从这个局面开始</n-button>
        </div>
      </n-tab-pane>

      <n-tab-pane name="fen" tab="FEN">
        <div class="pane">
          <n-input v-model:value="fenText" type="textarea" :autosize="{ minRows: 2, maxRows: 4 }" placeholder="粘贴 FEN，如 3k5/9/9/9/9/9/9/9/4R4/4K4 w" />
          <div v-if="fenText && !fenHasSide">
            <span class="muted">FEN 没有写明行棋方，请选择：</span>
            <n-radio-group v-model:value="fenSide" size="small">
              <n-radio-button value="red">红走</n-radio-button>
              <n-radio-button value="black">黑走</n-radio-button>
            </n-radio-group>
          </div>
          <n-input v-model:value="name" placeholder="名称（可选）" size="small" />
          <n-button type="primary" :loading="busy" :disabled="!fenText.trim() || (!fenHasSide && !fenSide)" @click="startFen">导入</n-button>
        </div>
      </n-tab-pane>

      <n-tab-pane name="record" tab="棋谱">
        <div class="pane">
          <p class="muted">支持中文记谱（如“1. 炮二平五 马8进7”）、ICCS 着法（h2e2）、带 [FEN "..."] 标签的 PGN，以及本应用导出的学习数据。</p>
          <n-input v-model:value="recordText" type="textarea" :autosize="{ minRows: 6, maxRows: 14 }" placeholder="粘贴棋谱" />
          <div class="row">
            <n-button v-if="isTauri" size="small" :loading="busy" @click="run(importFromFile)">从文件导入…</n-button>
            <label v-else class="file">选择文件 <input type="file" accept=".pgn,.txt,.json,.fen" @change="readFile" /></label>
            <n-input v-model:value="name" placeholder="名称（可选）" size="small" style="max-width: 240px" />
            <n-button type="primary" :loading="busy" :disabled="!recordText.trim()" @click="startRecord">导入棋谱</n-button>
          </div>
        </div>
      </n-tab-pane>

      <n-tab-pane name="saved" tab="已保存">
        <div class="saved">
          <n-empty v-if="!saved.length" description="还没有保存的学习会话" />
          <div v-for="s in saved" :key="s.id" class="saved-row">
            <div class="saved-main" @click="run(() => openStudy(s.id))">
              <strong>{{ s.name }}</strong>
              <n-tag size="tiny" :bordered="false">{{ SOURCE[s.source] }}</n-tag>
              <span class="muted">{{ s.moves }} 着 · {{ new Date(s.updated_at).toLocaleString() }}</span>
            </div>
            <n-popconfirm @positive-click="remove(s.id)">
              <template #trigger><n-button size="tiny" quaternary type="error">删除</n-button></template>
              删除后无法恢复，确定删除“{{ s.name }}”？
            </n-popconfirm>
          </div>
        </div>
      </n-tab-pane>
    </n-tabs>
  </n-modal>
</template>

<style scoped>
.pane {
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-width: 560px;
  padding: 8px 0;
}
.footer,
.row {
  display: flex;
  gap: 10px;
  align-items: center;
  margin-top: 10px;
  flex-wrap: wrap;
}
.muted {
  color: var(--text-muted);
  font-size: 13px;
  margin: 0;
}
.file {
  font-size: 13px;
  cursor: pointer;
  color: var(--accent-blue);
}
.file input {
  display: none;
}
.saved {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 60vh;
  overflow: auto;
}
.saved-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  border-radius: 6px;
}
.saved-row:hover {
  background: var(--hover-bg);
}
.saved-main {
  display: flex;
  gap: 8px;
  align-items: center;
  cursor: pointer;
  flex: 1;
}
</style>
