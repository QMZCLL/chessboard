<script setup lang="ts">
// 一段连续着法：沿第一个子节点向下展开，分支紧跟在被替代的着法之后缩进显示，可以折叠。
import { computed } from "vue";
import { NDropdown, useDialog } from "naive-ui";
import type { ExplainStatus } from "../api/types";
import { concealed, deleteFrom, nodeOf, promote, selectNode, shownNotation, subtreeSize, wb } from "../state/workbench";
import { moveNumber } from "../utils/board";

defineOptions({ name: "MoveLine" });

const props = defineProps<{ start: number; depth: number }>();
const dialog = useDialog();

interface Item {
  id: number;
  alternatives: number[];
}

const items = computed<Item[]>(() => {
  const out: Item[] = [];
  let cur: number | undefined = props.start;
  while (cur !== undefined) {
    const node = nodeOf(cur);
    if (!node) break;
    const parent = nodeOf(node.parent);
    const alternatives = parent && parent.children[0] === cur ? parent.children.slice(1) : [];
    out.push({ id: cur, alternatives });
    cur = node.children[0];
  }
  return out;
});

const rootSide = computed(() => nodeOf(0)?.side_to_move ?? "red");
const collapsed = computed(() => props.depth > 0 && !!wb.collapsed[props.start]);
const branchSize = computed(() => subtreeSize(props.start));

function label(id: number) {
  const node = nodeOf(id)!;
  const parent = nodeOf(node.parent);
  if (!parent) return "";
  return moveNumber(node.ply, rootSide.value, parent.side_to_move);
}

function status(id: number): ExplainStatus | undefined {
  return wb.view?.explain_status[String(id)];
}

const STATUS_TEXT: Record<ExplainStatus, string> = {
  queued: "讲解排队中",
  basic: "已有规则讲解",
  partial: "讲解不完整，可重试",
  detailed: "引擎分析完整",
  failed: "引擎分析未完成",
};

function menu(id: number) {
  const node = nodeOf(id);
  const parent = nodeOf(node?.parent);
  const options = [];
  if (parent && parent.children[0] !== id) options.push({ label: "设为主线", key: "promote" });
  options.push({ label: `删除此着及之后（共 ${subtreeSize(id)} 着）`, key: "delete" });
  return options;
}

function onMenu(id: number, key: string) {
  if (key === "promote") void promote(id);
  if (key === "delete") {
    const n = subtreeSize(id);
    dialog.warning({
      title: "删除着法",
      content: `删除“${label(id)} ${shownNotation(id, "尚未揭示的一着")}”及之后共 ${n} 着（含其中的分支），对应的讲解与笔记一并删除，无法撤销。`,
      positiveText: "删除",
      negativeText: "取消",
      onPositiveClick: () => deleteFrom(id),
    });
  }
}

function toggle() {
  wb.collapsed[props.start] = !collapsed.value;
}
</script>

<template>
  <div class="line" :class="{ variation: depth > 0 }">
    <button v-if="depth > 0" type="button" class="branch-head" :aria-expanded="!collapsed" @click="toggle">
      <span class="caret">{{ collapsed ? "▸" : "▾" }}</span>
      分支 · 从 {{ label(start) }} {{ shownNotation(start) }} 开始 · {{ branchSize }} 着
    </button>
    <template v-if="!collapsed">
      <template v-for="item in items" :key="item.id">
        <div v-if="nodeOf(item.id)?.gap" class="gap" :class="{ active: wb.selected === item.id }" :data-node="item.id" @click="selectNode(item.id)">
          画面缺口：{{ nodeOf(item.id)?.gap_reason }}
        </div>
        <div
          v-else
          class="move focusable"
          :class="{ active: wb.selected === item.id, live: wb.view?.study.live_node === item.id }"
          :data-node="item.id"
          tabindex="0"
          role="button"
          :aria-current="wb.selected === item.id ? 'step' : undefined"
          @click="selectNode(item.id)"
          @keydown.enter.self.prevent="selectNode(item.id)"
          @keydown.space.self.prevent="selectNode(item.id)"
        >
          <span class="num">{{ label(item.id) }}</span>
          <span class="notation" :class="{ concealed: concealed.has(item.id) }" :aria-label="concealed.has(item.id) ? '尚未揭示的着法' : undefined">
            {{ shownNotation(item.id) }}
          </span>
          <span class="flags">
            <span v-if="nodeOf(item.id)?.starred" class="star" title="已收藏">★</span>
            <span v-if="nodeOf(item.id)?.note" class="note" title="有笔记">✎</span>
            <span v-if="wb.view?.study.live_node === item.id" class="live-tag">实时</span>
            <span v-if="status(item.id)" class="dot" :class="status(item.id)" :title="STATUS_TEXT[status(item.id)!]" />
          </span>
          <n-dropdown trigger="click" :options="menu(item.id)" size="small" @select="(k: string) => onMenu(item.id, k)">
            <button type="button" class="more" title="更多操作" aria-label="更多操作" @click.stop>⋯</button>
          </n-dropdown>
        </div>
        <MoveLine v-for="alt in item.alternatives" :key="'alt' + alt" :start="alt" :depth="depth + 1" />
      </template>
    </template>
  </div>
</template>

<style scoped>
.line.variation {
  margin: 2px 0 var(--sp-1) 10px;
  padding-left: var(--sp-2);
  border-left: 2px solid var(--border-strong);
}
.branch-head {
  display: block;
  width: 100%;
  text-align: left;
  border: none;
  background: none;
  font: inherit;
  font-size: 12px;
  color: var(--text-muted);
  padding: 2px 4px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.branch-head:hover {
  background: var(--hover-bg);
}
.branch-head:focus-visible {
  outline: 2px solid var(--accent-blue);
}
.caret {
  display: inline-block;
  width: 1em;
}
.move {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-radius: var(--radius);
  cursor: pointer;
  font-size: var(--fs-body);
  line-height: 1.5;
  transition: background-color 0.12s ease;
}
.variation .move {
  font-size: var(--fs-group);
  color: var(--text-secondary);
}
.move:hover {
  background: var(--hover-bg);
}
.move.active {
  background: var(--selected-bg);
  box-shadow: inset 3px 0 0 var(--accent-blue);
  color: var(--text-primary);
  font-weight: 600;
}
.num {
  min-width: 2.6em;
  color: var(--text-muted);
  font-size: 12.5px;
  font-variant-numeric: tabular-nums;
}
.notation.concealed {
  color: var(--text-muted);
  letter-spacing: 4px;
}
.notation {
  flex: 1;
  font-family: var(--piece-font);
  font-size: 16px;
  letter-spacing: 1.5px;
}
.flags {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.star {
  color: var(--accent-amber);
}
.note {
  color: var(--text-muted);
}
.live-tag {
  font-size: 11px;
  padding: 0 4px;
  border-radius: 4px;
  background: var(--accent-red);
  color: #fff;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.dot.queued {
  border: 1.5px solid var(--text-muted);
}
.dot.basic {
  background: var(--accent-blue);
  opacity: 0.6;
}
.dot.detailed {
  background: var(--accent-green);
}
.dot.partial {
  border: 2px solid var(--accent-amber);
}
.dot.failed {
  background: var(--accent-amber);
}
.more {
  visibility: hidden;
  border: none;
  background: none;
  font: inherit;
  padding: 0 4px;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 4px;
}
.move:hover .more,
.move.active .more,
.move:focus-within .more,
.more:focus-visible {
  visibility: visible;
}
.more:focus-visible {
  outline: 2px solid var(--accent-blue);
}
.gap {
  margin: var(--sp-1) 0;
  padding: var(--sp-1) var(--sp-2);
  font-size: 12px;
  color: var(--accent-amber);
  border: 1px dashed var(--accent-amber);
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.gap.active {
  background: var(--selected-bg);
}
</style>
