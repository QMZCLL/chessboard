<script setup lang="ts">
// 导入多盘棋局到复盘收件箱：先粘贴或选择文件（可以多选，一个文件里可以有多盘），预览之后再导入。
// 预览写明每盘的着数、解析停在哪一步、是否与已有的棋局重复，以及按“我的名字”判断的我方与依据；
// 比对不上的要先选红方或黑方才能导入；重复的、不能导入的默认不勾选。
import { computed, reactive, ref, watch } from "vue";
import { NAlert, NButton, NCheckbox, NInput, NModal, NRadioButton, NRadioGroup, useMessage } from "naive-ui";
import { api, errorMessage, isTauri, pickRecordPaths } from "../api/backend";
import type { Color, InboxCandidate, InboxSource } from "../api/types";
import { loadInbox } from "../state/inbox";
import { wb } from "../state/workbench";

const show = defineModel<boolean>("show", { required: true });
const message = useMessage();
const text = ref("");
/** 网页版选择的文件（读出的文字）；桌面应用中由后台读取路径 */
const files = ref<{ name: string; text: string }[]>([]);
const paths = ref<string[]>([]);
const busy = ref(false);
const error = ref<string | null>(null);
const preview = ref<{ games: InboxCandidate[]; notes: string[] } | null>(null);
/** 每盘的选择：是否导入、我方 */
const picks = reactive<Record<number, { take: boolean; side: Color | null }>>({});

watch(show, (open) => {
  if (!open) return;
  text.value = "";
  files.value = [];
  paths.value = [];
  preview.value = null;
  error.value = null;
});

const stem = (name: string) => name.replace(/\.[^.]+$/, "");
async function readFiles(ev: Event) {
  const list = Array.from((ev.target as HTMLInputElement).files ?? []);
  files.value = await Promise.all(list.map(async (f) => ({ name: stem(f.name), text: await f.text() })));
}
async function chooseFiles() {
  paths.value = await pickRecordPaths();
}
const sourceCount = computed(() => (text.value.trim() ? 1 : 0) + files.value.length + paths.value.length);

async function runPreview() {
  const sources: InboxSource[] = [];
  if (text.value.trim()) sources.push({ kind: "text", name: null, text: text.value });
  for (const f of files.value) sources.push({ kind: "text", name: f.name, text: f.text });
  for (const p of paths.value) sources.push({ kind: "file", path: p });
  busy.value = true;
  try {
    const p = await api.inboxPreview(sources);
    for (const k of Object.keys(picks)) delete picks[Number(k)];
    for (const g of p.games) picks[g.index] = { take: !g.error && !g.duplicate, side: g.side };
    preview.value = p;
    error.value = null;
  } catch (e) {
    error.value = errorMessage(e);
  } finally {
    busy.value = false;
  }
}

const chosen = computed(() => (preview.value?.games ?? []).filter((g) => picks[g.index]?.take));
/** 勾选了、但还没有选我方的盘数 */
const missingSide = computed(() => chosen.value.filter((g) => !picks[g.index].side).length);

async function runImport() {
  const games = chosen.value.map((g) => ({ text: g.text, name: g.name, user_side: picks[g.index].side as Color }));
  busy.value = true;
  try {
    const r = await api.inboxImport(games);
    message.success(r.text);
    if (r.skipped.length) message.warning(r.skipped.slice(0, 3).join("；"));
    show.value = false;
    wb.main = "home";
    await loadInbox();
  } catch (e) {
    error.value = errorMessage(e);
  } finally {
    busy.value = false;
  }
}

const SIDE_LABEL: Record<Color, string> = { red: "红方", black: "黑方" };
</script>

<template>
  <n-modal v-model:show="show" preset="card" title="导入棋局到复盘收件箱" style="width: min(760px, 96vw)" data-testid="inbox-dialog">
    <template v-if="!preview">
      <p class="muted">一次导入多盘自己下的棋：粘贴棋谱，或选择文件（可以多选，一个文件里有多盘时按标签分开）。支持 PGN 风格的文字棋谱、中文记谱与 ICCS。</p>
      <n-input v-model:value="text" type="textarea" :autosize="{ minRows: 6, maxRows: 14 }" placeholder="粘贴一盘或多盘棋谱" data-testid="inbox-text" />
      <div class="row">
        <n-button v-if="isTauri" size="small" @click="chooseFiles">选择文件…</n-button>
        <label v-else class="file">选择文件 <input type="file" multiple accept=".pgn,.txt" data-testid="inbox-files" @change="readFiles" /></label>
        <span v-if="files.length || paths.length" class="muted">已选 {{ files.length + paths.length }} 个文件</span>
        <span class="grow" />
        <n-button type="primary" :loading="busy" :disabled="!sourceCount" data-testid="inbox-preview" @click="runPreview">预览</n-button>
      </div>
    </template>

    <template v-else>
      <p class="muted">设置中的“我的名字”用来判断哪一方是你；比对不上的请选择。出题只看你这一方的着法。</p>
      <n-alert v-for="n in preview.notes" :key="n" type="warning" :show-icon="false" class="gap" data-testid="inbox-note">{{ n }}</n-alert>
      <ol class="games">
        <li v-for="g in preview.games" :key="g.index" class="game" :data-inbox-candidate="g.index">
          <n-checkbox
            :checked="picks[g.index].take"
            :disabled="!!g.error"
            :aria-label="`导入第 ${g.index + 1} 盘`"
            data-testid="inbox-take"
            @update:checked="(v: boolean) => (picks[g.index].take = v)"
          />
          <div class="info">
            <div class="name">
              第 {{ g.index + 1 }} 盘 · {{ g.name }}
              <span v-if="g.date" class="muted">· {{ g.date }}</span>
              <span v-if="g.plies" class="muted">· {{ g.plies }} 着</span>
            </div>
            <p v-if="g.error" class="error" data-testid="inbox-error">不能导入：{{ g.error }}</p>
            <p v-if="g.duplicate" class="warn" data-testid="inbox-duplicate">{{ g.duplicate }}，默认不导入</p>
            <p v-if="g.stopped" class="muted">{{ g.stopped }}</p>
            <div v-if="!g.error" class="side">
              <span class="muted">我执</span>
              <n-radio-group :value="picks[g.index].side" size="small" @update:value="(v: Color) => (picks[g.index].side = v)">
                <n-radio-button v-for="c in ['red', 'black'] as const" :key="c" :value="c" :data-testid="`inbox-side-${c}`">{{ SIDE_LABEL[c] }}</n-radio-button>
              </n-radio-group>
              <span class="muted reason" data-testid="inbox-reason">{{ g.side_reason }}</span>
            </div>
          </div>
        </li>
      </ol>
      <div class="row">
        <n-button size="small" quaternary @click="preview = null">返回修改</n-button>
        <span class="grow" />
        <span v-if="missingSide" class="warn" data-testid="inbox-missing-side">还有 {{ missingSide }} 盘没有选择我方</span>
        <n-button type="primary" :loading="busy" :disabled="!chosen.length || !!missingSide" data-testid="inbox-import" @click="runImport">
          加入收件箱（{{ chosen.length }} 盘）
        </n-button>
      </div>
    </template>
    <n-alert v-if="error" type="error" :show-icon="false" class="gap">{{ error }}</n-alert>
  </n-modal>
</template>

<style scoped>
.row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-top: var(--sp-2);
}
.grow {
  flex: 1;
}
.games {
  list-style: none;
  margin: var(--sp-2) 0 0;
  padding: 0;
  max-height: 56vh;
  overflow: auto;
  display: grid;
  gap: 6px;
}
.game {
  display: flex;
  align-items: flex-start;
  gap: var(--sp-2);
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
}
.info {
  min-width: 0;
  flex: 1;
}
.name {
  font-weight: 600;
}
.info p {
  margin: 2px 0;
  line-height: 1.6;
}
.side {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 4px;
}
.reason {
  font-size: 12px;
}
.muted {
  color: var(--text-muted);
}
.warn {
  color: var(--accent-amber);
}
.error {
  color: var(--accent-red);
}
.gap {
  margin-top: 8px;
}
.file {
  font-size: 13px;
  color: var(--accent-blue);
  cursor: pointer;
}
.file input {
  display: none;
}
</style>
