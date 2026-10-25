<script setup lang="ts">
// 中间棋盘区：上方说明现在看的是哪一个局面，下方是固定的导航与演示工具栏。
// 棋盘尺寸按实际可用空间计算；方向键只在棋盘工作区生效，弹窗、设置与输入控件优先处理键盘。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { NButton, NButtonGroup, NDropdown, NTag, NTooltip } from "naive-ui";
import Icon from "./Icon.vue";
import { openComparison, trialChoice } from "../state/compare";
import XiangqiBoard from "./XiangqiBoard.vue";
import {
  addPreviewAsBranch,
  boardContext,
  displayFen,
  displayLastMove,
  displayLegal,
  displaySide,
  endTrial,
  exitPreview,
  flipBoard,
  goRelative,
  playMove,
  previewBackLabel,
  selectedNode,
  startTrial,
  stepPreview,
  study,
  thinkMarks,
  thinking,
  wb,
} from "../state/workbench";
import { sideLabel } from "../utils/board";
import { layout, ui } from "../state/layout";

const orientation = computed(() => study.value?.orientation ?? "red_bottom");
const detailHere = computed(() => (wb.detail?.node.id === wb.selected ? wb.detail : null));
const status = computed(() => (wb.preview || thinking.value ? null : detailHere.value?.status));
const inCheck = computed(() => !wb.preview && !thinking.value && detailHere.value?.in_check);
const interactive = computed(() => !wb.preview && status.value !== "checkmate" && status.value !== "stalemate");

const preview = computed(() => wb.preview);
const atStart = computed(() => !!preview.value && preview.value.index === 0);
const previewArrow = computed(() => (preview.value && preview.value.index > 0 ? displayLastMove.value : null));
// 演示起点的下一着；先想后讲时为三级提示中选看的着法
const nextArrow = computed(() =>
  atStart.value && preview.value!.moves.length ? preview.value!.moves[0] : thinking.value ? (thinkMarks.value?.next ?? null) : null,
);
const lastMove = computed(() => (wb.preview ? null : displayLastMove.value));
const marks = computed(() => (atStart.value ? preview.value!.marks : null));
// 先想后讲时只显示提示标出的棋子与关系（二级提示起）
const highlights = computed(
  () => marks.value?.squares ?? (wb.preview ? [] : thinking.value ? (thinkMarks.value?.squares ?? []) : (wb.highlight?.squares ?? [])),
);
const arrows = computed(
  () => marks.value?.arrows ?? (wb.preview ? [] : thinking.value ? (thinkMarks.value?.arrows ?? []) : (wb.highlight?.arrows ?? [])),
);
const shown = computed(() => Math.max(1, preview.value?.index ?? 0));
/** 演示到最后一着：主要操作从“下一步”变为返回 */
const atEnd = computed(() => !!preview.value && preview.value.index >= preview.value.moves.length);
const saveOptions = computed(() => [{ label: `保存整条变化（${preview.value?.moves.length ?? 0} 着）`, key: "all" }]);

// 浏览已有棋谱时才需要“从这里试走”：局面后面还有着法
const canTrial = computed(() => !wb.preview && !thinking.value && !wb.trial && !!selectedNode.value?.children.length);

// 棋盘按容器的实际宽高计算尺寸（612:672 的比例）
const wrap = ref<HTMLElement | null>(null);
const size = ref({ width: 0, height: 0 });
let observer: ResizeObserver | null = null;
const boardStyle = computed(() => {
  const { width, height } = size.value;
  if (!width || !height) return {};
  const w = Math.min(width, (height * 612) / 672);
  // 圆角与棋盘边框（视图中 14）按比例缩放，阴影才能贴合边框
  return { width: `${Math.floor(w)}px`, height: `${Math.floor((w * 672) / 612)}px`, borderRadius: `${(14 * w) / 612}px` };
});

const board = ref<InstanceType<typeof XiangqiBoard> | null>(null);

/** 当前焦点所在的界面会自己处理键盘时，棋谱导航让路。 */
function boardKeysAllowed(e: KeyboardEvent): boolean {
  if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return false;
  if (Object.values(wb.dialogs).some(Boolean)) return false;
  // 确认框等由组件库直接弹出的对话框打开时，焦点可能仍在页面上
  if (document.querySelector(".n-modal-mask, .n-drawer-mask")) return false;
  const t = e.target as HTMLElement | null;
  if (!t || t === document.body) return true;
  if (t.isContentEditable) return false;
  const interactiveSelector = [
    "input",
    "textarea",
    "select",
    "[contenteditable='true']",
    "[role='textbox']",
    "[role='combobox']",
    "[role='listbox']",
    "[role='slider']",
    "[role='separator']",
    "[role='radiogroup']",
    "[role='tablist']",
    "[role='menu']",
    "[role='dialog']",
    ".n-radio-group",
    ".n-tabs-nav",
    ".n-select",
    ".n-input-number",
    ".n-dropdown-menu",
    ".n-popover",
    ".n-modal",
    ".n-drawer",
  ].join(",");
  return !t.closest(interactiveSelector);
}

function onKey(e: KeyboardEvent) {
  if (!boardKeysAllowed(e)) return;
  if (e.key === "ArrowLeft") {
    if (wb.preview) stepPreview(-1);
    else goRelative(-1);
    e.preventDefault();
  } else if (e.key === "ArrowRight") {
    if (wb.preview) stepPreview(1);
    else goRelative(1);
    e.preventDefault();
  } else if (e.key === "Escape") {
    // 逐层退出：先收起棋谱浮层，再取消棋子选择，再退出演示，最后取消依据高亮
    if (layout.value === "medium" && ui.treeOpen) {
      ui.treeOpen = false;
      return;
    }
    if (board.value?.clearSelection()) return;
    if (wb.preview) exitPreview();
    else if (wb.focus || wb.highlight) {
      wb.focus = null;
      wb.highlight = null;
    }
  }
}

onMounted(() => {
  window.addEventListener("keydown", onKey);
  if (wrap.value) {
    observer = new ResizeObserver(([entry]) => {
      size.value = { width: entry.contentRect.width, height: entry.contentRect.height };
    });
    observer.observe(wrap.value);
  }
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  observer?.disconnect();
});
</script>

<template>
  <section class="area" :class="boardContext?.kind">
    <div class="context" data-testid="board-context">
      <span v-if="displaySide && !wb.preview" class="side" :class="displaySide">{{ sideLabel(displaySide) }}走</span>
      <span class="context-text">{{ boardContext?.text }}</span>
      <span v-if="boardContext?.hint" class="context-hint">· {{ boardContext.hint }}</span>
      <n-tag v-if="status === 'checkmate'" type="error" size="small" :bordered="false">已被将死</n-tag>
      <n-tag v-else-if="status === 'stalemate'" type="error" size="small" :bordered="false">困毙（无子可走，判负）</n-tag>
      <n-tag v-else-if="inCheck" type="warning" size="small" :bordered="false">正被将军</n-tag>
      <n-tag v-if="!wb.preview && detailHere && detailHere.repetition > 1" size="small" :bordered="false">局面重复 {{ detailHere.repetition }} 次</n-tag>
      <!-- 演示时，返回放在写明演示对象的说明旁边 -->
      <n-tooltip v-if="preview">
        <template #trigger>
          <n-button
            class="context-back"
            size="small"
            :type="atEnd ? 'primary' : 'default'"
            :secondary="!atEnd"
            data-testid="preview-back"
            @click="exitPreview"
          >
            <template #icon><Icon name="back" /></template>{{ previewBackLabel }}
          </n-button>
        </template>
        键盘 Esc
      </n-tooltip>
    </div>

    <div v-if="preview && preview.notation.length" class="steps" aria-label="演示的着法">
      <button
        v-for="(n, i) in preview.notation"
        :key="i"
        type="button"
        class="step"
        :class="{ done: i < preview.index, current: i === preview.index - 1 }"
        @click="preview.index = i + 1"
      >
        {{ n }}
      </button>
    </div>

    <div ref="wrap" class="board-wrap">
      <div class="board-box" :style="boardStyle">
        <XiangqiBoard
          v-if="displayFen"
          ref="board"
          :fen="displayFen"
          :orientation="orientation"
          :legal-moves="displayLegal"
          :last-move="lastMove"
          :preview-arrow="previewArrow"
          :next-arrow="nextArrow"
          :highlights="highlights"
          :arrows="arrows"
          :danger="wb.illegal?.squares ?? []"
          :interactive="interactive"
          @move="playMove"
        />
      </div>
    </div>

    <div v-if="wb.illegal" class="illegal" role="alert">不能这样走：{{ wb.illegal.message }}</div>

    <div class="controls" data-testid="board-controls">
      <template v-if="preview">
        <n-button-group v-if="preview.moves.length" size="medium">
          <n-tooltip>
            <template #trigger>
              <n-button :disabled="preview.index === 0" aria-label="起点" @click="preview.index = 0">
                <template #icon><Icon name="chevrons-left" /></template>
              </n-button>
            </template>
            回到演示起点
          </n-tooltip>
          <n-tooltip>
            <template #trigger>
              <n-button :disabled="preview.index === 0" @click="stepPreview(-1)"><template #icon><Icon name="chevron-left" /></template>上一步</n-button>
            </template>
            键盘 ←
          </n-tooltip>
          <n-tooltip>
            <template #trigger>
              <n-button
                icon-placement="right"
                :type="atEnd ? 'default' : 'primary'"
                :disabled="atEnd"
                data-testid="preview-next"
                @click="stepPreview(1)"
              >
                <template #icon><Icon name="chevron-right" /></template>下一步
              </n-button>
            </template>
            键盘 →
          </n-tooltip>
          <n-tooltip>
            <template #trigger>
              <n-button :disabled="preview.index === preview.moves.length" aria-label="终点" @click="preview.index = preview.moves.length">
                <template #icon><Icon name="chevrons-right" /></template>
              </n-button>
            </template>
            演示到最后一着
          </n-tooltip>
        </n-button-group>
        <span v-else class="controls-note">落子前的局面只用于查看依据，不能在这里走子</span>
        <span v-if="preview.moves.length" class="divider" />
        <!-- 保存范围写在按钮上；保存整条变化放在旁边的菜单里 -->
        <n-button-group v-if="preview.moves.length" size="medium">
          <n-button quaternary @click="addPreviewAsBranch(false)"><template #icon><Icon name="save" /></template>保存已演示的 {{ shown }} 着</n-button>
          <n-dropdown v-if="shown < preview.moves.length" trigger="click" :options="saveOptions" @select="addPreviewAsBranch(true)">
            <n-button quaternary aria-label="更多保存方式"><template #icon><Icon name="chevron-down" /></template></n-button>
          </n-dropdown>
        </n-button-group>
      </template>
      <template v-else>
        <n-button-group size="medium">
          <n-tooltip>
            <template #trigger>
              <n-button :disabled="!selectedNode || selectedNode.parent === null" @click="goRelative(-1)">
                <template #icon><Icon name="chevron-left" /></template>上一步
              </n-button>
            </template>
            键盘 ←
          </n-tooltip>
          <n-tooltip>
            <template #trigger>
              <n-button icon-placement="right" :disabled="!selectedNode?.children.length" @click="goRelative(1)">
                <template #icon><Icon name="chevron-right" /></template>下一步
              </n-button>
            </template>
            键盘 →（沿主线）
          </n-tooltip>
        </n-button-group>
        <span v-if="wb.trial || canTrial" class="divider" />
        <!-- 试走中：对比我的走法是主要操作，结束试走是次要操作 -->
        <n-button
          v-if="wb.trial && trialChoice"
          size="medium"
          type="primary"
          data-testid="compare-trial"
          @click="openComparison(trialChoice.origin, trialChoice.mine)"
        >
          <template #icon><Icon name="compare" /></template>对比我的走法
        </n-button>
        <n-button v-if="wb.trial" size="medium" secondary @click="endTrial"><template #icon><Icon name="back" /></template>结束试走，返回原局面</n-button>
        <n-tooltip v-else-if="canTrial">
          <template #trigger><n-button size="medium" secondary @click="startTrial"><template #icon><Icon name="branch" /></template>从这里试走</n-button></template>
          在这个局面走出自己的着法，走法保存在分支中
        </n-tooltip>
        <span class="divider" />
        <n-button size="medium" quaternary @click="flipBoard"><template #icon><Icon name="flip" /></template>翻转棋盘</n-button>
      </template>
    </div>
  </section>
</template>

<style scoped>
.area {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  min-height: 0;
  padding: var(--sp-2) var(--sp-3) var(--sp-3);
  gap: var(--sp-3);
}
/* 棋盘上方的状态条：浮在纸面上的细长卡片 */
.context {
  width: 100%;
  max-width: 760px;
  min-height: 36px;
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
  padding: 5px var(--sp-3) 5px 6px;
  border-radius: 999px;
  background: var(--panel);
  box-shadow: var(--shadow-1);
  font-size: var(--fs-group);
  transition: background-color 0.2s ease, box-shadow 0.2s ease;
}
.preview .context,
.before .context,
.think .context {
  background: color-mix(in srgb, var(--accent-blue) 9%, var(--panel));
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-blue) 45%, transparent), var(--shadow-1);
}
.trial .context {
  background: color-mix(in srgb, var(--accent-primary) 9%, var(--panel));
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 45%, transparent), var(--shadow-1);
}
.context-text {
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: 0.02em;
}
.context-hint {
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
.context-back {
  margin-left: auto;
}
.controls-note {
  padding: 0 var(--sp-2);
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
.side {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-weight: 600;
  padding: 2px 10px 2px 7px;
  border-radius: 999px;
  font-size: var(--fs-aux);
}
/* 行棋方前的小圆点，形状与颜色同时区分 */
.side::before {
  content: "";
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: inset 0 -1px 0 rgba(0, 0, 0, 0.25);
}
.side.red {
  background: var(--tag-red-bg);
  color: var(--accent-red);
}
.side.black {
  background: var(--tag-gray-bg);
  color: var(--text-primary);
}
.steps {
  width: 100%;
  max-width: 760px;
  display: flex;
  flex-wrap: wrap;
  gap: 2px var(--sp-1);
  font-family: var(--piece-font);
  margin-top: calc(var(--sp-1) * -1);
}
.step {
  cursor: pointer;
  border: none;
  background: none;
  font: inherit;
  font-size: var(--fs-body);
  color: var(--text-muted);
  padding: 0 6px;
  border-radius: var(--radius-sm);
  transition: background-color 0.15s ease, color 0.15s ease;
}
.step:hover {
  background: var(--hover-bg);
}
.step:focus-visible {
  outline: 2px solid var(--accent-blue);
}
.step.done {
  color: var(--text-primary);
}
.step.current {
  background: var(--tag-blue-bg);
  color: var(--accent-blue);
}
.board-wrap {
  flex: 1;
  min-height: 0;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 2px 0 6px;
}
/* 棋盘是画面的中心：投影让它浮在纸面上；演示与试走时外圈加一道颜色 */
.board-box {
  box-shadow: var(--board-shadow);
  transition: box-shadow 0.25s ease;
}
.preview .board-box,
.before .board-box {
  box-shadow: 0 0 0 3px var(--panel), 0 0 0 5.5px var(--accent-blue), var(--board-shadow);
}
.trial .board-box {
  box-shadow: 0 0 0 3px var(--panel), 0 0 0 5.5px var(--accent-primary), var(--board-shadow);
}
.illegal {
  color: var(--accent-red);
  font-size: var(--fs-group);
  padding: 4px var(--sp-3);
  border-radius: 999px;
  background: var(--tag-red-bg);
}
/* 棋盘下方的操作条：按钮略收窄，常用宽度下保持一行 */
.controls :deep(.n-button) {
  padding: 0 12px;
}
.controls {
  display: flex;
  gap: 6px;
  align-items: center;
  flex-wrap: wrap;
  justify-content: center;
  min-height: var(--control-h);
  padding: 4px 6px;
  border-radius: var(--radius-lg);
  background: var(--panel);
  box-shadow: var(--shadow-1);
}
.divider {
  width: 1px;
  height: 18px;
  background: var(--border);
}
</style>
