<script setup lang="ts">
// 摆棋与识别结果校正：点击放置所选棋子，右键或橡皮擦清除；行棋方必须明确选择。
import { ref, watch } from "vue";
import { NButton, NInput, NRadioButton, NRadioGroup, NSpace, NTag } from "naive-ui";
import XiangqiBoard from "./XiangqiBoard.vue";
import { api, errorMessage } from "../api/backend";
import type { Color, Orientation, Square, ValidateResponse } from "../api/types";
import { EMPTY_KINGS_BOARD, PALETTE, START_BOARD, boardToFen, isRedPiece, parseBoard, pieceName, placementOf, sideOfFen } from "../utils/board";

const props = withDefaults(
  defineProps<{
    board: string;
    side: Color | null;
    orientation: Orientation;
    marks?: Square[];
    changed?: Square[];
    showOrientation?: boolean;
  }>(),
  { marks: () => [], changed: () => [], showOrientation: false },
);

const emit = defineEmits<{
  (e: "update:board", v: string): void;
  (e: "update:side", v: Color | null): void;
  (e: "update:orientation", v: Orientation): void;
  (e: "validity", ok: boolean): void;
}>();

const tool = ref<string>("eraser");
const fenText = ref("");
const result = ref<ValidateResponse | null>(null);
const checkError = ref<string | null>(null);
let timer: number | undefined;

function onEdit(sq: Square, erase: boolean) {
  const map = parseBoard(props.board);
  if (erase || tool.value === "eraser") map.delete(sq);
  else map.set(sq, tool.value);
  emit("update:board", boardToFen(map));
}

function applyFenText() {
  const text = fenText.value.trim();
  if (!text) return;
  emit("update:board", placementOf(text));
  const side = sideOfFen(text);
  if (side) emit("update:side", side);
}

async function validate() {
  try {
    const resp = await api.validatePosition(props.board, props.side);
    result.value = resp;
    checkError.value = null;
    emit("validity", !resp.format_error && !!resp.report?.ok && !resp.needs_side);
  } catch (e) {
    checkError.value = errorMessage(e);
    emit("validity", false);
  }
}

watch(
  () => [props.board, props.side],
  () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(validate, 200);
  },
  { immediate: true },
);

function issueSquares(): Square[] {
  return result.value?.report?.issues.flatMap((i) => (i.severity === "error" ? i.squares : [])) ?? [];
}
</script>

<template>
  <div class="editor">
    <div class="board">
      <XiangqiBoard
        :fen="board"
        :orientation="orientation"
        editor
        :marks="marks"
        :highlights="changed"
        :danger="issueSquares()"
        @edit="onEdit"
      />
    </div>
    <div class="side-panel">
      <div class="palette">
        <button
          v-for="p in PALETTE"
          :key="p"
          class="chip"
          :class="{ red: isRedPiece(p), active: tool === p }"
          :title="(isRedPiece(p) ? '红' : '黑') + pieceName(p)"
          @click="tool = p"
        >
          {{ pieceName(p) }}
        </button>
        <button class="chip eraser" :class="{ active: tool === 'eraser' }" title="橡皮擦（也可右键清除）" @click="tool = 'eraser'">擦</button>
      </div>
      <n-space size="small">
        <n-button size="tiny" @click="emit('update:board', EMPTY_KINGS_BOARD)">只留将帅</n-button>
        <n-button size="tiny" @click="emit('update:board', START_BOARD)">标准开局</n-button>
      </n-space>

      <div class="field">
        <div class="label">轮到哪一方走（必选）</div>
        <n-radio-group :value="side" size="small" @update:value="(v: Color) => emit('update:side', v)">
          <n-radio-button value="red">红走</n-radio-button>
          <n-radio-button value="black">黑走</n-radio-button>
        </n-radio-group>
      </div>
      <div v-if="showOrientation" class="field">
        <div class="label">棋盘方向（画面中哪一方在下）</div>
        <n-radio-group :value="orientation" size="small" @update:value="(v: Orientation) => emit('update:orientation', v)">
          <n-radio-button value="red_bottom">红方在下</n-radio-button>
          <n-radio-button value="black_bottom">黑方在下</n-radio-button>
        </n-radio-group>
      </div>

      <div class="field">
        <div class="label">粘贴 FEN</div>
        <n-input v-model:value="fenText" size="small" placeholder="如 3k5/9/9/9/9/9/9/9/4R4/4K4 w" @keydown.enter="applyFenText" />
        <n-button size="tiny" style="margin-top: 4px" @click="applyFenText">应用</n-button>
      </div>

      <div class="checks">
        <p v-if="checkError" class="error">{{ checkError }}</p>
        <p v-else-if="result?.format_error" class="error">{{ result.format_error }}</p>
        <template v-else-if="result?.report">
          <p v-for="i in result.report.issues" :key="i.message" :class="i.severity">{{ i.message }}</p>
          <p v-if="result.needs_side" class="warning">请选择轮到哪一方走</p>
          <p v-else-if="result.report.ok" class="ok">
            局面成立
            <n-tag v-if="result.report.in_check" size="tiny" type="warning" :bordered="false">行棋方正被将军</n-tag>
          </p>
        </template>
        <p v-if="marks.length" class="hint">蓝色虚线圈：识别置信度较低，请核对。</p>
        <p v-if="changed.length" class="hint">橙色圈：与上一个确认局面不同的位置。</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.editor {
  display: grid;
  grid-template-columns: minmax(260px, 1fr) 240px;
  gap: 16px;
}
.board {
  max-height: 60vh;
  aspect-ratio: 612 / 672;
}
.side-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.palette {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 6px;
}
/* 调色板中的棋子与棋盘上的棋子同一质感：木质侧边、浅色棋面、刻线 */
.chip {
  font-family: var(--piece-font);
  font-size: 18px;
  font-weight: 700;
  aspect-ratio: 1;
  border-radius: 50%;
  border: none;
  background: radial-gradient(circle at 38% 32%, var(--piece-face-1), var(--piece-face-2) 78%);
  box-shadow: inset 0 0 0 2px var(--piece-rim-1), inset 0 0 0 3.5px var(--piece-rim-2), inset 0 0 0 5px var(--piece-face-2),
    inset 0 0 0 6px var(--piece-black), 0 2px 3px var(--piece-shadow);
  color: var(--piece-black);
  cursor: pointer;
  transition: transform 0.12s ease;
}
.chip:hover {
  transform: translateY(-1px);
}
.chip.red {
  color: var(--piece-red);
  box-shadow: inset 0 0 0 2px var(--piece-rim-1), inset 0 0 0 3.5px var(--piece-rim-2), inset 0 0 0 5px var(--piece-face-2),
    inset 0 0 0 6px var(--piece-red), 0 2px 3px var(--piece-shadow);
}
.chip.eraser {
  font-size: 14px;
  background: var(--panel-raised);
  box-shadow: inset 0 0 0 1px var(--border-strong);
  color: var(--text-secondary);
}
.chip.active {
  outline: 2.5px solid var(--accent-blue);
  outline-offset: 2px;
}
.label {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 4px;
}
.checks p {
  margin: 2px 0;
  font-size: 13px;
}
.error {
  color: var(--accent-red);
}
.warning {
  color: var(--accent-amber);
}
.ok {
  color: var(--accent-green);
}
.hint {
  color: var(--text-muted);
}
</style>
