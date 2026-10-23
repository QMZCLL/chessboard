<script setup lang="ts">
// 由状态驱动的棋盘：根据传入的局面、高亮与箭头渲染，不在内部保存棋局。
import { computed, ref, watch } from "vue";
import type { Color, Move, Orientation, Square } from "../api/types";
import { fileLabel, fileOf, isRedPiece, parseBoard, pieceName, rankOf, squareOf } from "../utils/board";

const props = withDefaults(
  defineProps<{
    fen: string;
    orientation: Orientation;
    legalMoves?: Move[];
    lastMove?: Move | null;
    highlights?: Square[];
    arrows?: Move[];
    previewArrow?: Move | null;
    danger?: Square[];
    marks?: Square[];
    interactive?: boolean;
    editor?: boolean;
  }>(),
  {
    legalMoves: () => [],
    lastMove: null,
    highlights: () => [],
    arrows: () => [],
    previewArrow: null,
    danger: () => [],
    marks: () => [],
    interactive: false,
    editor: false,
  },
);

const emit = defineEmits<{
  (e: "move", mv: Move): void;
  (e: "edit", sq: Square, erase: boolean): void;
}>();

const CELL = 60;
const PAD = 66;
const WIDTH = PAD * 2 + CELL * 8;
const HEIGHT = PAD * 2 + CELL * 9;

const flipped = computed(() => props.orientation === "black_bottom");
const board = computed(() => parseBoard(props.fen));
const sideToMove = computed<Color>(() => (props.fen.split(" ")[1] === "b" ? "black" : "red"));
const selected = ref<Square | null>(null);

watch(
  () => props.fen,
  () => (selected.value = null),
);

function xy(sq: Square) {
  const file = fileOf(sq);
  const rank = rankOf(sq);
  const col = flipped.value ? 8 - file : file;
  const row = flipped.value ? rank : 9 - rank;
  return { x: PAD + col * CELL, y: PAD + row * CELL };
}

const squares = computed(() => {
  const out: Square[] = [];
  for (let rank = 0; rank < 10; rank++) for (let file = 0; file < 9; file++) out.push(squareOf(file, rank));
  return out;
});

const pieces = computed(() =>
  [...board.value.entries()].map(([sq, ch]) => ({ sq, ch, red: isRedPiece(ch), name: pieceName(ch), ...xy(sq) })),
);

const targets = computed(() =>
  selected.value ? props.legalMoves.filter((m) => m.startsWith(selected.value!)).map((m) => m.slice(2, 4)) : [],
);

const bottomSide = computed<Color>(() => (flipped.value ? "black" : "red"));
const topSide = computed<Color>(() => (flipped.value ? "red" : "black"));

function colLabel(col: number, side: Color) {
  const file = flipped.value ? 8 - col : col;
  return fileLabel(side, file);
}

function arrowPath(mv: Move) {
  const a = xy(mv.slice(0, 2));
  const b = xy(mv.slice(2, 4));
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = Math.hypot(dx, dy) || 1;
  const shrink = 22;
  return {
    x1: a.x + (dx / len) * 12,
    y1: a.y + (dy / len) * 12,
    x2: b.x - (dx / len) * shrink,
    y2: b.y - (dy / len) * shrink,
  };
}

function onSquare(sq: Square, ev: MouseEvent) {
  if (props.editor) {
    emit("edit", sq, ev.button === 2);
    return;
  }
  if (!props.interactive) return;
  const piece = board.value.get(sq);
  const own = piece && (isRedPiece(piece) ? "red" : "black") === sideToMove.value;
  if (selected.value && selected.value !== sq && !own) {
    emit("move", `${selected.value}${sq}`);
    selected.value = null;
    return;
  }
  if (own) {
    selected.value = selected.value === sq ? null : sq;
    return;
  }
  selected.value = null;
}

const palaceLines = computed(() => {
  const lines: { x1: number; y1: number; x2: number; y2: number }[] = [];
  for (const top of [0, 7]) {
    const a = { x: PAD + 3 * CELL, y: PAD + top * CELL };
    const b = { x: PAD + 5 * CELL, y: PAD + (top + 2) * CELL };
    lines.push({ x1: a.x, y1: a.y, x2: b.x, y2: b.y });
    lines.push({ x1: b.x, y1: a.y, x2: a.x, y2: b.y });
  }
  return lines;
});
</script>

<template>
  <svg
    class="xq-board"
    :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
    role="img"
    aria-label="象棋棋盘"
    @contextmenu.prevent
  >
    <defs>
      <marker id="xq-arrow" viewBox="0 0 10 10" refX="6" refY="5" markerWidth="4" markerHeight="4" orient="auto">
        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--accent-amber)" />
      </marker>
      <marker id="xq-arrow-preview" viewBox="0 0 10 10" refX="6" refY="5" markerWidth="4" markerHeight="4" orient="auto">
        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--accent-blue)" />
      </marker>
      <radialGradient id="xq-piece" cx="40%" cy="35%" r="70%">
        <stop offset="0%" stop-color="#fff6e3" />
        <stop offset="100%" stop-color="#e9cf9b" />
      </radialGradient>
    </defs>
    <rect class="frame" x="2" y="2" :width="WIDTH - 4" :height="HEIGHT - 4" rx="10" />
    <rect class="field" :x="PAD" :y="PAD" :width="CELL * 8" :height="CELL * 9" />
    <!-- 横线 -->
    <line v-for="r in 10" :key="'h' + r" class="grid" :x1="PAD" :x2="PAD + CELL * 8" :y1="PAD + (r - 1) * CELL" :y2="PAD + (r - 1) * CELL" />
    <!-- 竖线：两侧贯通，中间在河界断开 -->
    <template v-for="c in 9" :key="'v' + c">
      <line v-if="c === 1 || c === 9" class="grid" :x1="PAD + (c - 1) * CELL" :x2="PAD + (c - 1) * CELL" :y1="PAD" :y2="PAD + CELL * 9" />
      <template v-else>
        <line class="grid" :x1="PAD + (c - 1) * CELL" :x2="PAD + (c - 1) * CELL" :y1="PAD" :y2="PAD + CELL * 4" />
        <line class="grid" :x1="PAD + (c - 1) * CELL" :x2="PAD + (c - 1) * CELL" :y1="PAD + CELL * 5" :y2="PAD + CELL * 9" />
      </template>
    </template>
    <line v-for="(l, i) in palaceLines" :key="'p' + i" class="grid" v-bind="l" />
    <text class="river" :x="PAD + CELL * 2" :y="PAD + CELL * 4.5">楚 河</text>
    <text class="river" :x="PAD + CELL * 6" :y="PAD + CELL * 4.5">汉 界</text>
    <!-- 纵线编号：各自从本方右侧数起 -->
    <text v-for="c in 9" :key="'t' + c" class="coord" :x="PAD + (c - 1) * CELL" :y="PAD - 46">{{ colLabel(c - 1, topSide) }}</text>
    <text v-for="c in 9" :key="'b' + c" class="coord" :x="PAD + (c - 1) * CELL" :y="PAD + CELL * 9 + 46">{{ colLabel(c - 1, bottomSide) }}</text>

    <!-- 上一步 -->
    <template v-if="lastMove">
      <rect v-for="sq in [lastMove.slice(0, 2), lastMove.slice(2, 4)]" :key="'lm' + sq" class="last" :x="xy(sq).x - 29" :y="xy(sq).y - 29" width="58" height="58" rx="8" />
    </template>
    <!-- 讲解高亮 -->
    <circle v-for="sq in highlights" :key="'hl' + sq" class="hl" :cx="xy(sq).x" :cy="xy(sq).y" r="30" />
    <circle v-for="sq in danger" :key="'dg' + sq" class="danger" :cx="xy(sq).x" :cy="xy(sq).y" r="30" />
    <circle v-for="sq in marks" :key="'mk' + sq" class="mark" :cx="xy(sq).x" :cy="xy(sq).y" r="30" />

    <!-- 棋子 -->
    <g v-for="p in pieces" :key="p.sq + p.ch" class="piece" :class="{ red: p.red, picked: selected === p.sq }" :transform="`translate(${p.x},${p.y})`">
      <circle r="26" class="disc" />
      <circle r="21.5" class="ring" />
      <text class="glyph" dy="0.36em">{{ p.name }}</text>
    </g>

    <!-- 可走点 -->
    <template v-for="sq in targets" :key="'tg' + sq">
      <circle v-if="board.get(sq)" class="capture-hint" :cx="xy(sq).x" :cy="xy(sq).y" r="28" />
      <circle v-else class="move-hint" :cx="xy(sq).x" :cy="xy(sq).y" r="8" />
    </template>

    <!-- 箭头 -->
    <line v-for="(a, i) in arrows" :key="'ar' + i" class="arrow" v-bind="arrowPath(a)" marker-end="url(#xq-arrow)" />
    <line v-if="previewArrow" class="arrow preview" v-bind="arrowPath(previewArrow)" marker-end="url(#xq-arrow-preview)" />

    <!-- 点击区域 -->
    <rect
      v-for="sq in squares"
      :key="'hit' + sq"
      class="hit"
      :class="{ active: interactive || editor }"
      :x="xy(sq).x - CELL / 2"
      :y="xy(sq).y - CELL / 2"
      :width="CELL"
      :height="CELL"
      :data-square="sq"
      @mousedown="onSquare(sq, $event)"
    />
  </svg>
</template>

<style scoped>
.xq-board {
  width: 100%;
  height: 100%;
  max-height: 100%;
  user-select: none;
  display: block;
}
.frame {
  fill: var(--board-frame);
  stroke: var(--board-line);
  stroke-width: 2;
}
.field {
  fill: var(--board-field);
  stroke: var(--board-line);
  stroke-width: 3;
}
.grid {
  stroke: var(--board-line);
  stroke-width: 1.4;
}
.river {
  font-family: var(--piece-font);
  font-size: 30px;
  fill: var(--board-line);
  opacity: 0.75;
  text-anchor: middle;
  dominant-baseline: middle;
  letter-spacing: 8px;
}
.coord {
  font-size: 16px;
  fill: var(--board-coord);
  text-anchor: middle;
  dominant-baseline: middle;
}
.last {
  fill: var(--board-last);
}
.hl {
  fill: none;
  stroke: var(--accent-amber);
  stroke-width: 4;
}
.danger {
  fill: none;
  stroke: var(--accent-red);
  stroke-width: 4;
  stroke-dasharray: 6 4;
}
.mark {
  fill: none;
  stroke: var(--accent-blue);
  stroke-width: 3;
  stroke-dasharray: 3 4;
}
.disc {
  fill: url(#xq-piece);
  stroke: #8a6a3a;
  stroke-width: 1.5;
  filter: drop-shadow(0 2px 2px rgba(0, 0, 0, 0.35));
}
.ring {
  fill: none;
  stroke: #2b2b2b;
  stroke-width: 1.4;
}
.piece.red .ring {
  stroke: #b3261e;
}
.glyph {
  font-family: var(--piece-font);
  font-size: 30px;
  font-weight: 700;
  text-anchor: middle;
  fill: #1f1f1f;
}
.piece.red .glyph {
  fill: #b3261e;
}
.piece.picked .disc {
  stroke: var(--accent-blue);
  stroke-width: 4;
}
.move-hint {
  fill: var(--accent-blue);
  opacity: 0.55;
  pointer-events: none;
}
.capture-hint {
  fill: none;
  stroke: var(--accent-blue);
  stroke-width: 3;
  pointer-events: none;
}
.arrow {
  stroke: var(--accent-amber);
  stroke-width: 8;
  stroke-linecap: round;
  opacity: 0.85;
  pointer-events: none;
}
.arrow.preview {
  stroke: var(--accent-blue);
}
.hit {
  fill: transparent;
}
.hit.active {
  cursor: pointer;
}
</style>
