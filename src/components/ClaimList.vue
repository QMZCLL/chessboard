<script setup lang="ts">
// 讲解要点列表。点击一段只选中它并突出显示依据；演示变化使用独立的按钮。
// 行的 Enter/空格只在行本身获得焦点时处理（.self 在 .prevent 之前），行内按钮的按键保持按钮自己的作用。
// 依据在落子前的局面上时，不把标记画在落子后的棋盘上，而是提示起点并提供查看入口。
import { computed } from "vue";
import { NButton } from "naive-ui";
import Icon from "./Icon.vue";
import type { Claim, Evidence } from "../api/types";
import { canDemo, demoEvidence, focusEvidence, viewEvidenceBoard, wb } from "../state/workbench";

const props = defineProps<{ claims: Claim[]; evidence: Evidence[]; scope: string }>();

function evidenceOf(claim: Claim): Evidence[] {
  return claim.evidence.map((id) => props.evidence.find((e) => e.id === id)).filter((e): e is Evidence => !!e);
}

function keyOf(i: number) {
  return `${props.scope}:${i}`;
}

/** 选中时默认查看的依据：优先选能在棋盘上标出的。 */
function primary(claim: Claim): Evidence | null {
  const list = evidenceOf(claim);
  return list.find((e) => e.squares.length || e.arrows.length) ?? list[0] ?? null;
}

function lineEvidence(claim: Claim): Evidence | null {
  return evidenceOf(claim).find(canDemo) ?? null;
}

const focusedIndex = computed(() => {
  const f = wb.focus;
  if (!f || !f.key.startsWith(`${props.scope}:`)) return -1;
  return Number(f.key.slice(props.scope.length + 1));
});

function focusedEvidence(claim: Claim): Evidence | null {
  const id = wb.focus?.evidenceId;
  return evidenceOf(claim).find((e) => e.id === id) ?? null;
}

function badge(claim: Claim): { text: string; cls: string } | null {
  const kinds = evidenceOf(claim).map((e) => e.certainty);
  if (kinds.includes("tentative")) return { text: "位置判断", cls: "tentative" };
  if (kinds.includes("supported")) return { text: "变化支持", cls: "supported" };
  return null;
}

function select(claim: Claim, i: number) {
  focusEvidence(keyOf(i), primary(claim));
}

function demo(claim: Claim, i: number) {
  const ev = lineEvidence(claim);
  if (!ev) return;
  focusEvidence(keyOf(i), ev);
  demoEvidence(ev);
}

function hasMarks(ev: Evidence | null): ev is Evidence {
  return !!ev && (ev.squares.length > 0 || ev.arrows.length > 0);
}

/** 依据标出的棋子在落子前的局面上（与棋盘此刻是否在演示无关）。 */
function onParent(ev: Evidence | null): boolean {
  return hasMarks(ev) && ev.on === "parent";
}

/** 早期保存的依据没有记录所在局面：不画在棋盘上，并说明原因。 */
function boardUnknown(ev: Evidence | null): boolean {
  return hasMarks(ev) && !ev.on;
}

function short(text: string) {
  return text.length > 16 ? `${text.slice(0, 16)}…` : text;
}
</script>

<template>
  <ul class="claims">
    <li
      v-for="(c, i) in claims"
      :key="i"
      class="claim focusable"
      :class="{ active: focusedIndex === i }"
      tabindex="0"
      role="button"
      :aria-pressed="focusedIndex === i"
      @click="select(c, i)"
      @keydown.enter.self.prevent="select(c, i)"
      @keydown.space.self.prevent="select(c, i)"
    >
      <span class="text">{{ c.text }}</span>
      <span v-if="badge(c)" class="badge" :class="badge(c)!.cls">{{ badge(c)!.text }}</span>
      <n-button v-if="lineEvidence(c)" text size="tiny" class="demo" type="info" @click.stop="demo(c, i)">
        <template #icon><Icon name="play" :size="11" /></template>演示变化
      </n-button>

      <div v-if="focusedIndex === i" class="detail" @click.stop>
        <div v-if="evidenceOf(c).length > 1" class="chips" role="group" aria-label="这段讲解的依据">
          <button
            v-for="e in evidenceOf(c)"
            :key="e.id"
            type="button"
            class="chip"
            :class="{ on: wb.focus?.evidenceId === e.id }"
            :title="e.text"
            @click="focusEvidence(keyOf(i), e)"
          >
            {{ short(e.text) }}
          </button>
        </div>
        <p v-if="onParent(focusedEvidence(c))" class="before">
          这条依据在落子前的局面上。
          <n-button v-if="!wb.preview" text size="tiny" type="info" @click="viewEvidenceBoard(focusedEvidence(c)!)">在落子前局面查看</n-button>
        </p>
        <p v-else-if="boardUnknown(focusedEvidence(c))" class="before" data-testid="evidence-board-unknown">
          这条依据来自旧版讲解，没有记录所在的局面，因此不在棋盘上标出；重新生成 AI 讲解后可以标出。
        </p>
      </div>
    </li>
  </ul>
</template>

<style scoped>
.claims {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
}
/* 每一段前有一个墨点；选中的一段成为带蓝色描边的卡片 */
.claim {
  position: relative;
  padding: 6px var(--sp-3) 6px 26px;
  border-radius: var(--radius);
  line-height: var(--lh-body);
  font-size: var(--fs-body);
  cursor: pointer;
  transition: background-color 0.15s ease, box-shadow 0.15s ease;
}
.claim::before {
  content: "";
  position: absolute;
  left: 11px;
  top: calc(6px + 0.85em - 3px);
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--text-muted);
  opacity: 0.55;
  transition: background-color 0.15s ease, opacity 0.15s ease;
}
.claim:hover {
  background: var(--hover-bg);
}
.claim.active {
  background: var(--selected-bg);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent-blue) 55%, transparent);
}
.claim.active::before {
  background: var(--accent-blue);
  opacity: 1;
}
.badge {
  margin-left: 6px;
  font-size: 11px;
  padding: 0 5px;
  border-radius: 4px;
  white-space: nowrap;
}
.badge.supported {
  background: var(--tag-blue-bg);
  color: var(--accent-blue);
}
.badge.tentative {
  background: var(--tag-gray-bg);
  color: var(--text-muted);
}
.demo {
  margin-left: 6px;
  font-size: 12px;
}
.detail {
  cursor: default;
  margin-top: var(--sp-1);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-1);
}
.chip {
  font: inherit;
  font-size: 12px;
  padding: 1px var(--sp-2);
  border-radius: 10px;
  border: 1px solid var(--border-strong);
  background: var(--panel);
  color: var(--text-secondary);
  cursor: pointer;
}
.chip.on {
  border-color: var(--accent-blue);
  color: var(--accent-blue);
  background: var(--tag-blue-bg);
}
.chip:focus-visible {
  outline: 2px solid var(--accent-blue);
}
.before {
  margin: var(--sp-1) 0 0;
  font-size: var(--fs-aux);
  color: var(--text-muted);
}
</style>
