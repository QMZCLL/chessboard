<script setup lang="ts">
// 屏幕连接：选择窗口 → 框选并校准棋盘 → 确认起始局面与行棋方 → 持续记录；无法解释的画面变化交由用户确认。
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { NAlert, NButton, NCheckbox, NEmpty, NInput, NModal, NRadioButton, NRadioGroup, NSpin, NTag, useMessage } from "naive-ui";
import BoardEditor from "./BoardEditor.vue";
import { api, errorMessage, isTauri } from "../api/backend";
import type { Color, Orientation, Rect, Snapshot, WindowInfo } from "../api/types";
import { openStudy, selectNode, study, wb } from "../state/workbench";

const message = useMessage();
const step = ref<"windows" | "calibrate">("windows");
const windows = ref<WindowInfo[]>([]);
const query = ref("");
const loading = ref(false);
const snapshot = ref<Snapshot | null>(null);
const rect = ref<Rect>({ x: 0, y: 0, w: 0, h: 0 });

// 确认起始局面 / 画面变化
const board = ref("");
const side = ref<Color | null>(null);
const orientation = ref<Orientation>("red_bottom");
const valid = ref(false);
const attach = ref(false);
const name = ref("");

const state = computed(() => wb.capture);
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return q ? windows.value.filter((w) => `${w.title} ${w.app_name}`.toLowerCase().includes(q)) : windows.value;
});

async function loadWindows() {
  loading.value = true;
  try {
    windows.value = await api.listWindows();
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    loading.value = false;
  }
}

watch(
  () => wb.dialogs.capture,
  (open) => {
    if (open && (state.value.state === "idle" || state.value.state === "stopped")) {
      step.value = "windows";
      void loadWindows();
    }
  },
);

watch(
  state,
  (s) => {
    if (s.state === "awaiting_start" || s.state === "awaiting_confirmation") {
      board.value = s.observed.board;
      side.value = s.observed.suggested_side;
      if (s.state === "awaiting_start") orientation.value = s.observed.orientation ?? "red_bottom";
    }
  },
  { immediate: true },
);

async function chooseWindow(w: WindowInfo) {
  loading.value = true;
  try {
    snapshot.value = await api.captureSnapshot(w.id);
    const s = snapshot.value;
    rect.value = s.suggested ?? { x: s.width * 0.2, y: s.height * 0.1, w: s.width * 0.6, h: s.height * 0.8 };
    step.value = "calibrate";
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    loading.value = false;
  }
}

async function refreshSnapshot() {
  if (snapshot.value) await chooseWindow(snapshot.value.window);
}

// ---------- 框选 ----------
// 截图按原始宽高比缩放显示；方框坐标始终以截图像素为单位
const wrap = ref<HTMLDivElement | null>(null);
const available = ref(0);
const observer = new ResizeObserver((entries) => {
  available.value = entries[0]?.contentRect.width ?? 0;
});
watch(wrap, (el, old) => {
  if (old) observer.unobserve(old);
  if (el) observer.observe(el);
});
onBeforeUnmount(() => observer.disconnect());

const stageSize = computed(() => {
  const s = snapshot.value;
  if (!s) return { width: 0, height: 0 };
  const maxHeight = window.innerHeight * 0.6;
  const width = Math.min(available.value, (maxHeight * s.width) / s.height);
  return { width, height: (width * s.height) / s.width };
});
const scale = computed(() => (snapshot.value ? stageSize.value.width / snapshot.value.width : 1));
let drag: { mode: "move" | "resize"; sx: number; sy: number; start: Rect } | null = null;

const rectStyle = computed(() => ({
  left: `${rect.value.x * scale.value}px`,
  top: `${rect.value.y * scale.value}px`,
  width: `${rect.value.w * scale.value}px`,
  height: `${rect.value.h * scale.value}px`,
}));

function startDrag(e: MouseEvent, mode: "move" | "resize") {
  drag = { mode, sx: e.clientX, sy: e.clientY, start: { ...rect.value } };
  window.addEventListener("mousemove", onDrag);
  window.addEventListener("mouseup", endDrag);
  e.preventDefault();
}

function onDrag(e: MouseEvent) {
  const s = snapshot.value;
  if (!drag || !s) return;
  const dx = (e.clientX - drag.sx) / scale.value;
  const dy = (e.clientY - drag.sy) / scale.value;
  const r = { ...drag.start };
  if (drag.mode === "move") {
    r.x = Math.min(Math.max(0, r.x + dx), s.width - r.w);
    r.y = Math.min(Math.max(0, r.y + dy), s.height - r.h);
  } else {
    r.w = Math.min(Math.max(90, r.w + dx), s.width - r.x);
    r.h = Math.min(Math.max(100, r.h + dy), s.height - r.y);
  }
  rect.value = r;
}

function endDrag() {
  drag = null;
  window.removeEventListener("mousemove", onDrag);
  window.removeEventListener("mouseup", endDrag);
}
onBeforeUnmount(endDrag);

const dots = computed(() => {
  const out: { x: number; y: number }[] = [];
  for (let r = 0; r < 10; r++) for (let c = 0; c < 9; c++) out.push({ x: ((c + 0.5) / 9) * 100, y: ((r + 0.5) / 10) * 100 });
  return out;
});

async function start() {
  const s = snapshot.value;
  if (!s) return;
  loading.value = true;
  try {
    const r = rect.value;
    await api.startCapture(s.window.id, { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) });
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    loading.value = false;
  }
}

async function confirmStart() {
  if (!side.value) return;
  loading.value = true;
  try {
    const target = attach.value && study.value ? { study_id: study.value.id, node_id: wb.selected } : null;
    const view = await api.confirmCaptureStart({ board: board.value, side: side.value, orientation: orientation.value, name: name.value || null, attach: target });
    await openStudy(view.study.id, view.study.live_node ?? 0);
    wb.dialogs.capture = false;
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    loading.value = false;
  }
}

async function resolve(action: "accept" | "new_study" | "dismiss") {
  loading.value = true;
  try {
    if (action === "dismiss") await api.resolveCapture({ action });
    else {
      if (!side.value) return;
      await api.resolveCapture({ action, board: board.value, side: side.value });
    }
    const s = wb.capture;
    if (s.state === "tracking" && s.study_id !== study.value?.id) await openStudy(s.study_id, s.node_id);
    else if (s.state === "tracking") await selectNode(s.node_id);
    wb.dialogs.capture = false;
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    loading.value = false;
  }
}

async function stop() {
  await api.stopCapture().catch((e) => message.error(errorMessage(e)));
  step.value = "windows";
  void loadWindows();
}

async function relocate() {
  await api.relocateCapture().catch((e) => message.error(errorMessage(e)));
}

// 开发桥的模拟画面
const simFen = ref("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR");
const simOrientation = ref<Orientation>("red_bottom");
async function showSim() {
  await api.devShowBoard(simFen.value.trim().split(/\s+/)[0] || null, simOrientation.value).catch((e) => message.error(errorMessage(e)));
}
</script>

<template>
  <n-modal v-model:show="wb.dialogs.capture" preset="card" title="屏幕连接" style="width: min(1000px, 96vw)">
    <n-alert v-if="!wb.status?.recognition.available" type="warning" :show-icon="false" class="gap">
      屏幕识别不可用：{{ wb.status?.recognition.detail }}。仍可通过摆棋、FEN 或棋谱开始学习。
    </n-alert>

    <!-- 未连接：选择窗口与框选 -->
    <template v-if="state.state === 'idle' || state.state === 'stopped'">
      <n-alert v-if="state.state === 'stopped'" type="error" :show-icon="false" class="gap">上次连接已停止：{{ state.message }}</n-alert>
      <template v-if="step === 'windows'">
        <div class="row">
          <n-input v-model:value="query" size="small" placeholder="搜索窗口标题或程序名" clearable />
          <n-button size="small" :loading="loading" @click="loadWindows">刷新</n-button>
        </div>
        <div class="windows">
          <n-empty v-if="!filtered.length && !loading" description="没有找到窗口" />
          <div v-for="w in filtered" :key="w.id" class="window" @click="chooseWindow(w)">
            <strong>{{ w.title || "（无标题）" }}</strong>
            <span class="muted">{{ w.app_name }} · {{ w.width }}×{{ w.height }}</span>
          </div>
        </div>
      </template>
      <template v-else-if="snapshot">
        <p class="muted">
          拖动方框对准棋盘，拖右下角调整大小：让每个蓝点落在棋盘交叉点上（方框比最外侧交叉点各多出半格）。
          <span v-if="snapshot.note">{{ snapshot.note }}</span>
        </p>
        <div ref="wrap" class="stage-wrap">
        <div class="stage" :style="{ width: stageSize.width + 'px', height: stageSize.height + 'px' }">
          <img v-if="snapshot.image" :src="snapshot.image" alt="窗口截图" draggable="false" />
          <div v-else class="placeholder">（没有截图）</div>
          <div class="rect" :style="rectStyle" @mousedown="startDrag($event, 'move')">
            <span v-for="(d, i) in dots" :key="i" class="dot" :style="{ left: d.x + '%', top: d.y + '%' }" />
            <span class="handle" @mousedown.stop="startDrag($event, 'resize')" />
          </div>
        </div>
        </div>
        <div class="row end">
          <n-button size="small" @click="step = 'windows'">返回</n-button>
          <n-button size="small" @click="refreshSnapshot">重新截图</n-button>
          <n-button type="primary" size="small" :loading="loading" :disabled="!wb.status?.recognition.available" @click="start">开始连接</n-button>
        </div>
      </template>
    </template>

    <template v-else-if="state.state === 'locating'">
      <div class="center">
        <n-spin size="small" />
        <p>正在识别“{{ state.window.title }}”中的棋盘，等待画面稳定…</p>
        <p v-if="state.message" class="muted">{{ state.message }}</p>
        <n-button size="small" @click="stop">停止</n-button>
      </div>
    </template>

    <template v-else-if="state.state === 'awaiting_start'">
      <p>请核对识别出的局面，必要时直接在棋盘上修改，并选择<strong>轮到哪一方走</strong>。</p>
      <n-alert v-for="i in state.observed.issues" :key="i" type="warning" :show-icon="false" class="gap">{{ i }}</n-alert>
      <BoardEditor
        v-model:board="board"
        v-model:side="side"
        v-model:orientation="orientation"
        show-orientation
        :marks="state.observed.uncertain"
        @validity="(ok: boolean) => (valid = ok)"
      />
      <div class="row end">
        <n-checkbox v-if="study" v-model:checked="attach">接在当前学习会话的这一步之后（作为新的起点）</n-checkbox>
        <n-input v-if="!attach" v-model:value="name" size="small" placeholder="新会话名称（可选）" style="max-width: 220px" />
        <n-button size="small" @click="relocate">重新识别</n-button>
        <n-button size="small" @click="stop">停止</n-button>
        <n-button type="primary" size="small" :loading="loading" :disabled="!valid || !side" @click="confirmStart">确认并开始记录</n-button>
      </div>
    </template>

    <template v-else-if="state.state === 'awaiting_confirmation'">
      <n-alert type="warning" :show-icon="false" class="gap">{{ state.reason }}</n-alert>
      <p class="muted">已确认的棋谱不会被改动。可以校正画面局面后按它继续（会在棋谱中标记缺口），或暂不处理继续等待。</p>
      <BoardEditor v-model:board="board" v-model:side="side" :orientation="study?.orientation ?? 'red_bottom'" :marks="state.observed.uncertain" :changed="state.observed.changed" @validity="(ok: boolean) => (valid = ok)" />
      <div class="row end">
        <n-button size="small" @click="resolve('dismiss')">暂不处理</n-button>
        <n-button v-if="state.kind === 'new_game'" size="small" type="primary" :disabled="!valid || !side" :loading="loading" @click="resolve('new_study')">作为新的一局记录</n-button>
        <n-button size="small" :type="state.kind === 'new_game' ? 'default' : 'primary'" :disabled="!valid || !side" :loading="loading" @click="resolve('accept')">按此局面继续</n-button>
      </div>
    </template>

    <template v-else-if="state.state === 'tracking'">
      <div class="center">
        <n-tag type="success" :bordered="false">正在记录</n-tag>
        <p>正在跟踪“{{ state.window.title }}”。确认的走子会实时加入棋谱，并在后台生成讲解。</p>
        <p v-if="state.message" class="muted">{{ state.message }}</p>
        <div class="row">
          <n-button size="small" @click="stop">停止连接</n-button>
        </div>
      </div>
    </template>

    <div v-if="!isTauri" class="sim">
      <div class="muted">开发桥：模拟画面</div>
      <div class="row">
        <n-input v-model:value="simFen" size="small" placeholder="画面局面（标准方向 FEN）" />
        <n-radio-group v-model:value="simOrientation" size="small">
          <n-radio-button value="red_bottom">红在下</n-radio-button>
          <n-radio-button value="black_bottom">黑在下</n-radio-button>
        </n-radio-group>
        <n-button size="small" data-testid="sim-show" @click="showSim">显示</n-button>
      </div>
    </div>
  </n-modal>
</template>

<style scoped>
.gap {
  margin-bottom: 10px;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  margin: 8px 0;
}
.row.end {
  justify-content: flex-end;
}
.windows {
  max-height: 50vh;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.window {
  display: flex;
  flex-direction: column;
  padding: 6px 10px;
  border-radius: 6px;
  cursor: pointer;
}
.window:hover {
  background: var(--hover-bg);
}
.muted {
  color: var(--text-muted);
  font-size: 13px;
}
.stage-wrap {
  width: 100%;
  display: flex;
  justify-content: center;
}
.stage {
  position: relative;
  background: var(--panel-raised);
  overflow: hidden;
  user-select: none;
}
.stage img {
  width: 100%;
  height: 100%;
  display: block;
}
.placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
}
.rect {
  position: absolute;
  border: 2px solid var(--accent-blue);
  background: rgba(37, 99, 235, 0.08);
  cursor: move;
}
.dot {
  position: absolute;
  width: 6px;
  height: 6px;
  margin: -3px 0 0 -3px;
  border-radius: 50%;
  background: var(--accent-blue);
  pointer-events: none;
}
.handle {
  position: absolute;
  right: -7px;
  bottom: -7px;
  width: 14px;
  height: 14px;
  background: var(--accent-blue);
  border-radius: 3px;
  cursor: nwse-resize;
}
.center {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px;
  text-align: center;
}
.sim {
  margin-top: 16px;
  padding-top: 8px;
  border-top: 1px dashed var(--border-strong);
}
</style>
