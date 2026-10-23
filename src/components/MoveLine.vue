<script setup lang="ts">
// 一段连续着法：沿第一个子节点向下展开，分支紧跟在被替代的着法之后缩进显示。
import { computed } from "vue";
import { NDropdown } from "naive-ui";
import type { ExplainStatus } from "../api/types";
import { deleteFrom, nodeOf, promote, selectNode, wb } from "../state/workbench";
import { moveNumber } from "../utils/board";

defineOptions({ name: "MoveLine" });

const props = defineProps<{ start: number; depth: number }>();

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
  basic: "已有基本讲解",
  detailed: "详细讲解已完成",
  failed: "详细讲解未完成",
};

function menu(id: number) {
  const node = nodeOf(id);
  const parent = nodeOf(node?.parent);
  const options = [];
  if (parent && parent.children[0] !== id) options.push({ label: "设为主线", key: "promote" });
  options.push({ label: "删除此着及之后", key: "delete" });
  return options;
}

function onMenu(id: number, key: string) {
  if (key === "promote") void promote(id);
  if (key === "delete") void deleteFrom(id);
}
</script>

<template>
  <div class="line" :class="{ variation: depth > 0 }">
    <template v-for="item in items" :key="item.id">
      <div v-if="nodeOf(item.id)?.gap" class="gap" :class="{ active: wb.selected === item.id }" @click="selectNode(item.id)">
        画面缺口：{{ nodeOf(item.id)?.gap_reason }}
      </div>
      <div
          v-else
          class="move"
          :class="{ active: wb.selected === item.id, live: wb.view?.study.live_node === item.id }"
          :data-node="item.id"
          @click="selectNode(item.id)"
        >
          <span class="num">{{ label(item.id) }}</span>
          <span class="notation">{{ nodeOf(item.id)?.notation }}</span>
          <span class="flags">
            <span v-if="nodeOf(item.id)?.starred" class="star" title="已收藏">★</span>
            <span v-if="nodeOf(item.id)?.note" class="note" title="有笔记">✎</span>
            <span v-if="wb.view?.study.live_node === item.id" class="live-tag">实时</span>
            <span v-if="status(item.id)" class="dot" :class="status(item.id)" :title="STATUS_TEXT[status(item.id)!]" />
          </span>
          <n-dropdown trigger="click" :options="menu(item.id)" size="small" @select="(k: string) => onMenu(item.id, k)">
            <span class="more" title="更多操作" @click.stop>⋯</span>
          </n-dropdown>
        </div>
      <MoveLine v-for="alt in item.alternatives" :key="'alt' + alt" :start="alt" :depth="depth + 1" />
    </template>
  </div>
</template>

<style scoped>
.line.variation {
  margin: 2px 0 4px 10px;
  padding-left: 8px;
  border-left: 2px solid var(--border-strong);
}
.move {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 15px;
  line-height: 1.5;
}
.variation .move {
  font-size: 14px;
  color: var(--text-secondary);
}
.move:hover {
  background: var(--hover-bg);
}
.move.active {
  background: var(--selected-bg);
  color: var(--text-primary);
  font-weight: 600;
}
.num {
  min-width: 2.6em;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}
.notation {
  flex: 1;
  font-family: var(--piece-font);
  letter-spacing: 1px;
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
.dot.failed {
  background: var(--accent-amber);
}
.more {
  visibility: hidden;
  padding: 0 4px;
  color: var(--text-muted);
}
.move:hover .more,
.move.active .more {
  visibility: visible;
}
.gap {
  margin: 4px 0;
  padding: 4px 8px;
  font-size: 12px;
  color: var(--accent-amber);
  border: 1px dashed var(--accent-amber);
  border-radius: 6px;
  cursor: pointer;
}
.gap.active {
  background: var(--selected-bg);
}
</style>
