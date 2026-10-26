<script setup lang="ts">
// 学习档案：导出全部会话与复习卡片；导入前先预览会新增、保留与合并哪些内容；查看自动备份，或立即备份。
import { onMounted, ref } from "vue";
import { NButton, useMessage } from "naive-ui";
import { api, errorMessage, isTauri, pickOpenPath, pickSavePath } from "../api/backend";
import type { ArchiveSource, ArchiveSummary, BackupInfo } from "../api/types";
import { loadReviews } from "../state/review";
import { archivePreviewLines } from "../state/reviewList";

const message = useMessage();
const busy = ref(false);
const preview = ref<{ source: ArchiveSource; name: string; summary: ArchiveSummary } | null>(null);
const backups = ref<BackupInfo | null>(null);
const fileInput = ref<HTMLInputElement | null>(null);

async function run(task: () => Promise<void>) {
  busy.value = true;
  try {
    await task();
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    busy.value = false;
  }
}

function fileName() {
  const d = new Date();
  return `象棋学习档案-${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, "0")}${String(d.getDate()).padStart(2, "0")}.json`;
}

const exportArchive = () =>
  run(async () => {
    if (isTauri) {
      const path = await pickSavePath(fileName());
      if (!path) return;
      await api.exportArchiveToFile(path);
      message.success(`已导出到 ${path}`);
      return;
    }
    const text = await api.exportArchive();
    const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = fileName();
    a.click();
    URL.revokeObjectURL(url);
  });

const previewSource = (source: ArchiveSource, name: string) =>
  run(async () => {
    preview.value = { source, name, summary: await api.previewArchive(source) };
  });

async function chooseFile() {
  if (!isTauri) {
    fileInput.value?.click();
    return;
  }
  const path = await pickOpenPath();
  if (path) await previewSource({ kind: "file", path }, path);
}

async function readFile(e: Event) {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (file) await previewSource({ kind: "text", text: await file.text() }, file.name);
}

const confirmImport = () =>
  run(async () => {
    const p = preview.value;
    if (!p) return;
    const done = await api.importArchive(p.source);
    preview.value = null;
    await loadReviews();
    message.success(`已导入：新增 ${done.studies_new} 个会话、${done.cards_new} 张卡片，合并 ${done.cards_merge} 张`);
  });

const refreshBackups = () =>
  run(async () => {
    backups.value = await api.backupInfo();
  });

const backupNow = () =>
  run(async () => {
    backups.value = await api.backupNow();
    message.success("已备份当天的学习档案");
  });

const size = (bytes: number) => (bytes < 1024 * 1024 ? `${Math.max(1, Math.round(bytes / 1024))} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`);
const time = (ms: number) => new Date(ms).toLocaleString();

onMounted(refreshBackups);
</script>

<template>
  <div class="archive" data-testid="archive-panel">
    <p class="muted">
      学习档案包含全部学习会话（棋谱、分支、标注、练习记录与讲解）和复习卡片（历次复习、理解与下一次复习的安排），用于换设备恢复或备份。
    </p>
    <div class="row">
      <n-button size="small" :loading="busy" data-testid="archive-export" @click="exportArchive">导出学习档案</n-button>
      <n-button size="small" :disabled="busy" data-testid="archive-import" @click="chooseFile">导入学习档案…</n-button>
      <input ref="fileInput" type="file" accept=".json" class="hidden" data-testid="archive-file" @change="readFile" />
    </div>

    <div v-if="preview" class="preview" data-testid="archive-preview">
      <strong>导入前确认：{{ preview.name }}</strong>
      <ul>
        <li v-for="l in archivePreviewLines(preview.summary)" :key="l">{{ l }}</li>
      </ul>
      <div class="row">
        <n-button size="small" type="primary" :loading="busy" data-testid="archive-confirm" @click="confirmImport">确认导入</n-button>
        <n-button size="small" quaternary :disabled="busy" @click="preview = null">取消</n-button>
      </div>
    </div>

    <h4>自动备份</h4>
    <p class="muted">
      <template v-if="backups?.keep">每天第一次启动时备份一份（按 UTC 日期），保留最近 {{ backups.keep }} 份。</template>
      <template v-else>已关闭自动备份，可以在上方“自动备份份数”中开启。</template>
    </p>
    <p v-if="backups" class="muted path" data-testid="archive-backup-dir">位置：{{ backups.dir }}</p>
    <ul v-if="backups?.files.length" class="files" data-testid="archive-backups">
      <li v-for="f in backups.files" :key="f.name">{{ f.name }} <span class="muted">· {{ size(f.size) }} · {{ time(f.modified_at) }}</span></li>
    </ul>
    <p v-else class="muted">还没有备份。</p>
    <n-button size="small" :loading="busy" data-testid="archive-backup-now" @click="backupNow">立即备份</n-button>
  </div>
</template>

<style scoped>
.archive p {
  margin: 0 0 var(--sp-2);
  line-height: 1.6;
}
.row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-bottom: var(--sp-3);
}
.hidden {
  display: none;
}
.preview {
  margin-bottom: var(--sp-3);
  padding: var(--sp-3);
  border-radius: var(--radius-lg);
  box-shadow: inset 0 0 0 1px var(--border);
}
.preview ul,
.files {
  margin: var(--sp-2) 0;
  padding-left: 18px;
  line-height: 1.7;
}
h4 {
  margin: var(--sp-4) 0 var(--sp-1);
  font-size: 14px;
}
.muted {
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
.path {
  word-break: break-all;
}
</style>
