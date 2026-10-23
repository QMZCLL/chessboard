<script setup lang="ts">
// 由状态驱动的棋盘：根据传入的局面、高亮与箭头渲染，不在内部保存棋局。
// 外观：木质边框与盘面（叠一层生成好的木纹）、双线外框、炮兵位记号；棋子由侧边、面、刻线与刻字组成。
import { computed, ref, watch } from "vue";
import type { Color, Move, Orientation, Square } from "../api/types";
import { fileLabel, fileOf, isRedPiece, parseBoard, pieceName, rankOf, squareOf } from "../utils/board";
import { woodTexture } from "../utils/woodTexture";

const props = withDefaults(
  defineProps<{
    fen: string;
    orientation: Orientation;
    legalMoves?: Move[];
    lastMove?: Move | null;
    highlights?: Square[];
    arrows?: Move[];
    previewArrow?: Move | null;
    /** 演示起点上即将走出的一着（虚线） */
    nextArrow?: Move | null;
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
    nextArrow: null,
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

/** 取消已选中的棋子；返回之前是否有选中（供 Esc 逐层退出使用）。 */
function clearSelection(): boolean {
  const had = selected.value !== null;
  selected.value = null;
  return had;
}
defineExpose({ clearSelection });

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

/** 炮位与兵卒位的“花”记号：交叉点四角的短折线，靠棋盘边缘的一侧不画。 */
const posMarks = computed(() => {
  const points: [number, number][] = [];
  for (const col of [1, 7]) for (const row of [2, 7]) points.push([col, row]);
  for (const col of [0, 2, 4, 6, 8]) for (const row of [3, 6]) points.push([col, row]);
  const g = 5;
  const l = 10;
  let d = "";
  for (const [col, row] of points) {
    const cx = PAD + col * CELL;
    const cy = PAD + row * CELL;
    for (const dx of [-1, 1]) {
      if ((col === 0 && dx < 0) || (col === 8 && dx > 0)) continue;
      for (const dy of [-1, 1]) {
        d += `M${cx + dx * g},${cy + dy * (g + l)}L${cx + dx * g},${cy + dy * g}L${cx + dx * (g + l)},${cy + dy * g}`;
      }
    }
  }
  return d;
});

/** 上一步的起点与落点：用四角括号标出，不遮挡棋子。 */
function cornerPath(sq: Square) {
  const { x, y } = xy(sq);
  const h = 28;
  const l = 9;
  let d = "";
  for (const dx of [-1, 1]) {
    for (const dy of [-1, 1]) {
      const cx = x + dx * h;
      const cy = y + dy * h;
      d += `M${cx},${cy - dy * l}L${cx},${cy}L${cx - dx * l},${cy}`;
    }
  }
  return d;
}

const texture = woodTexture();
const FRAME = 14;
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
        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--board-mark)" />
      </marker>
      <marker id="xq-arrow-preview" viewBox="0 0 10 10" refX="6" refY="5" markerWidth="4" markerHeight="4" orient="auto">
        <path d="M0,0 L10,5 L0,10 z" style="fill: var(--accent-blue)" />
      </marker>
      <linearGradient id="xq-frame" x1="0" y1="0" x2="0.35" y2="1">
        <stop offset="0%" style="stop-color: var(--board-frame-1)" />
        <stop offset="100%" style="stop-color: var(--board-frame-2)" />
      </linearGradient>
      <radialGradient id="xq-field" cx="45%" cy="40%" r="75%">
        <stop offset="0%" style="stop-color: var(--board-field-1)" />
        <stop offset="100%" style="stop-color: var(--board-field-2)" />
      </radialGradient>
      <!-- 棋子：侧边上亮下暗，面中心偏左上高光 -->
      <linearGradient id="xq-rim" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" style="stop-color: var(--piece-rim-1)" />
        <stop offset="100%" style="stop-color: var(--piece-rim-2)" />
      </linearGradient>
      <radialGradient id="xq-face" cx="38%" cy="32%" r="78%">
        <stop offset="0%" style="stop-color: var(--piece-face-1)" />
        <stop offset="100%" style="stop-color: var(--piece-face-2)" />
      </radialGradient>
      <radialGradient id="xq-shadow">
        <stop offset="0%" style="stop-color: var(--piece-shadow)" />
        <stop offset="60%" style="stop-color: var(--piece-shadow); stop-opacity: 0.45" />
        <stop offset="100%" style="stop-color: var(--piece-shadow); stop-opacity: 0" />
      </radialGradient>
      <radialGradient id="xq-halo">
        <stop offset="55%" style="stop-color: var(--board-mark); stop-opacity: 0.35" />
        <stop offset="100%" style="stop-color: var(--board-mark); stop-opacity: 0" />
      </radialGradient>
      <clipPath id="xq-clip-frame"><rect x="0" y="0" :width="WIDTH" :height="HEIGHT" rx="14" /></clipPath>
      <clipPath id="xq-clip-field">
        <rect :x="FRAME" :y="FRAME" :width="WIDTH - FRAME * 2" :height="HEIGHT - FRAME * 2" rx="4" />
      </clipPath>
    </defs>

    <!-- 边框与盘面 -->
    <rect class="frame" x="0" y="0" :width="WIDTH" :height="HEIGHT" rx="14" />
    <image v-if="texture" class="grain frame-grain" :href="texture" x="0" y="0" :width="WIDTH" :height="HEIGHT" preserveAspectRatio="none" clip-path="url(#xq-clip-frame)" />
    <rect class="frame-hi" x="1" y="1" :width="WIDTH - 2" :height="HEIGHT - 2" rx="13" />
    <rect class="field" :x="FRAME" :y="FRAME" :width="WIDTH - FRAME * 2" :height="HEIGHT - FRAME * 2" rx="4" />
    <image v-if="texture" class="grain" :href="texture" :x="FRAME" :y="FRAME" :width="WIDTH - FRAME * 2" :height="HEIGHT - FRAME * 2" preserveAspectRatio="none" clip-path="url(#xq-clip-field)" />
    <rect class="field-edge" :x="FRAME" :y="FRAME" :width="WIDTH - FRAME * 2" :height="HEIGHT - FRAME * 2" rx="4" />

    <!-- 双线外框 -->
    <rect class="border-outer" :x="PAD - 7" :y="PAD - 7" :width="CELL * 8 + 14" :height="CELL * 9 + 14" />
    <rect class="border-inner" :x="PAD" :y="PAD" :width="CELL * 8" :height="CELL * 9" />
    <!-- 横线 -->
    <line v-for="r in 8" :key="'h' + r" class="grid" :x1="PAD" :x2="PAD + CELL * 8" :y1="PAD + r * CELL" :y2="PAD + r * CELL" />
    <!-- 竖线：两侧由外框代替，中间在河界断开 -->
    <template v-for="c in 7" :key="'v' + c">
      <line class="grid" :x1="PAD + c * CELL" :x2="PAD + c * CELL" :y1="PAD" :y2="PAD + CELL * 4" />
      <line class="grid" :x1="PAD + c * CELL" :x2="PAD + c * CELL" :y1="PAD + CELL * 5" :y2="PAD + CELL * 9" />
    </template>
    <line v-for="(l, i) in palaceLines" :key="'p' + i" class="grid" v-bind="l" />
    <path class="pos-mark" :d="posMarks" />
    <text class="river" :x="PAD + CELL * 2" :y="PAD + CELL * 4.5">楚 河</text>
    <text class="river" :x="PAD + CELL * 6" :y="PAD + CELL * 4.5">汉 界</text>
    <!-- 纵线编号：各自从本方右侧数起 -->
    <text v-for="c in 9" :key="'t' + c" class="coord" :x="PAD + (c - 1) * CELL" :y="PAD - 40">{{ colLabel(c - 1, topSide) }}</text>
    <text v-for="c in 9" :key="'b' + c" class="coord" :x="PAD + (c - 1) * CELL" :y="PAD + CELL * 9 + 40">{{ colLabel(c - 1, bottomSide) }}</text>

    <!-- 上一步：落点加淡底，起点与落点都用角标 -->
    <template v-if="lastMove">
      <rect class="last-fill" :x="xy(lastMove.slice(2, 4)).x - 28" :y="xy(lastMove.slice(2, 4)).y - 28" width="56" height="56" rx="6" />
      <path v-for="(sq, i) in [lastMove.slice(0, 2), lastMove.slice(2, 4)]" :key="'lm' + sq" class="last" :class="{ from: i === 0 }" :d="cornerPath(sq)" />
    </template>
    <!-- 讲解高亮 -->
    <circle v-for="sq in highlights" :key="'hh' + sq" class="hl-halo" :cx="xy(sq).x" :cy="xy(sq).y" r="38" />
    <circle v-for="sq in highlights" :key="'hl' + sq" class="hl" :cx="xy(sq).x" :cy="xy(sq).y" r="30.5" />
    <circle v-for="sq in danger" :key="'dg' + sq" class="danger" :cx="xy(sq).x" :cy="xy(sq).y" r="30.5" />
    <circle v-for="sq in marks" :key="'mk' + sq" class="mark" :cx="xy(sq).x" :cy="xy(sq).y" r="30.5" />

    <!-- 棋子 -->
    <g v-for="p in pieces" :key="p.sq + p.ch" class="piece" :class="{ red: p.red, picked: selected === p.sq }" :transform="`translate(${p.x},${p.y})`">
      <ellipse class="shadow" cx="1.5" cy="4.5" rx="31" ry="30" />
      <g class="body">
        <circle r="27" class="rim" />
        <circle r="23.6" class="face" />
        <circle r="20" class="ring" />
        <text class="glyph-hi" dy="0.36em" y="1">{{ p.name }}</text>
        <text class="glyph" dy="0.36em">{{ p.name }}</text>
      </g>
    </g>

    <!-- 可走点 -->
    <template v-for="sq in targets" :key="'tg' + sq">
      <circle v-if="board.get(sq)" class="capture-hint" :cx="xy(sq).x" :cy="xy(sq).y" r="29.5" />
      <template v-else>
        <circle class="move-halo" :cx="xy(sq).x" :cy="xy(sq).y" r="13" />
        <circle class="move-hint" :cx="xy(sq).x" :cy="xy(sq).y" r="6.5" />
      </template>
    </template>

    <!-- 箭头 -->
    <line v-for="(a, i) in arrows" :key="'ar' + i" class="arrow" v-bind="arrowPath(a)" marker-end="url(#xq-arrow)" />
    <line v-if="previewArrow" class="arrow preview" v-bind="arrowPath(previewArrow)" marker-end="url(#xq-arrow-preview)" />
    <line v-if="nextArrow" class="arrow next" v-bind="arrowPath(nextArrow)" marker-end="url(#xq-arrow-preview)" />

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
  fill: url(#xq-frame);
}
.grain {
  opacity: var(--board-grain);
  pointer-events: none;
}
.frame-grain {
  opacity: calc(var(--board-grain) * 2.2);
}
/* 边框内外的细微倒角 */
.frame-hi {
  fill: none;
  stroke: rgba(255, 255, 255, 0.22);
  stroke-width: 1;
}
.field {
  fill: url(#xq-field);
}
.field-edge {
  fill: none;
  stroke: rgba(40, 20, 0, 0.45);
  stroke-width: 1.2;
}
.border-outer {
  fill: none;
  stroke: var(--board-line);
  stroke-width: 3;
}
.border-inner {
  fill: none;
  stroke: var(--board-line);
  stroke-width: 1.3;
}
.grid {
  stroke: var(--board-line);
  stroke-width: 1.2;
  opacity: 0.9;
}
.pos-mark {
  fill: none;
  stroke: var(--board-line);
  stroke-width: 1.2;
  opacity: 0.9;
}
.river {
  font-family: var(--piece-font);
  font-size: 34px;
  fill: var(--board-line);
  opacity: 0.62;
  text-anchor: middle;
  dominant-baseline: middle;
  letter-spacing: 14px;
}
/* 纵线编号在盘面边缘与底线棋子之间的空白里 */
.coord {
  font-family: var(--font-serif);
  font-size: 14px;
  fill: var(--board-coord);
  text-anchor: middle;
  dominant-baseline: middle;
}
.last-fill {
  fill: var(--board-last);
  opacity: 0.1;
}
.last {
  fill: none;
  stroke: var(--board-last);
  stroke-width: 2.6;
  stroke-linecap: round;
  stroke-linejoin: round;
  opacity: 0.9;
}
.last.from {
  opacity: 0.5;
}
.hl-halo {
  fill: url(#xq-halo);
  pointer-events: none;
}
.hl {
  fill: none;
  stroke: var(--board-mark);
  stroke-width: 3.2;
}
.danger {
  fill: none;
  stroke: var(--accent-red);
  stroke-width: 3.5;
  stroke-dasharray: 6 4;
}
.mark {
  fill: none;
  stroke: var(--accent-blue);
  stroke-width: 3;
  stroke-dasharray: 3 4;
}
.shadow {
  fill: url(#xq-shadow);
  transition: transform 0.15s ease, opacity 0.15s ease;
}
.body {
  transition: transform 0.15s ease;
}
.rim {
  fill: url(#xq-rim);
  stroke: var(--piece-edge);
  stroke-width: 1;
}
.face {
  fill: url(#xq-face);
  stroke: rgba(80, 50, 15, 0.35);
  stroke-width: 0.8;
}
.ring {
  fill: none;
  stroke: var(--piece-black);
  stroke-width: 1.3;
  opacity: 0.85;
}
.piece.red .ring {
  stroke: var(--piece-red);
}
.glyph,
.glyph-hi {
  font-family: var(--piece-font);
  font-size: 29px;
  font-weight: 700;
  text-anchor: middle;
}
.glyph {
  fill: var(--piece-black);
}
.piece.red .glyph {
  fill: var(--piece-red);
}
/* 刻字下缘的一道浅色，形成阴刻的感觉 */
.glyph-hi {
  fill: rgba(255, 255, 255, 0.7);
}
.piece.picked .body {
  transform: translateY(-4px) scale(1.04);
}
.piece.picked .shadow {
  transform: translate(2px, 5px) scale(1.06);
  opacity: 0.75;
}
.piece.picked .rim {
  stroke: var(--accent-blue);
  stroke-width: 3;
}
.move-halo {
  fill: var(--accent-blue);
  opacity: 0.16;
  pointer-events: none;
}
.move-hint {
  fill: var(--accent-blue);
  opacity: 0.75;
  pointer-events: none;
}
.capture-hint {
  fill: none;
  stroke: var(--accent-blue);
  stroke-width: 3;
  stroke-dasharray: 5 4;
  pointer-events: none;
}
.arrow {
  stroke: var(--board-mark);
  stroke-width: 7;
  stroke-linecap: round;
  opacity: 0.88;
  pointer-events: none;
}
.arrow.preview {
  stroke: var(--accent-blue);
}
.arrow.next {
  stroke: var(--accent-blue);
  stroke-width: 5;
  stroke-dasharray: 10 8;
  opacity: 0.75;
}
.hit {
  fill: transparent;
}
.hit.active {
  cursor: pointer;
}
</style>
