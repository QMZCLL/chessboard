<script setup lang="ts">
// 讲解要点列表：每条结论都关联证据，点击可在棋盘上高亮并回放相关变化。
import type { Claim, Evidence } from "../api/types";
import { showEvidence, wb } from "../state/workbench";

const props = defineProps<{ claims: Claim[]; evidence: Evidence[] }>();

function evidenceOf(claim: Claim): Evidence[] {
  return claim.evidence.map((id) => props.evidence.find((e) => e.id === id)).filter((e): e is Evidence => !!e);
}

function primary(claim: Claim): Evidence | undefined {
  return evidenceOf(claim)[0];
}

function active(claim: Claim) {
  return evidenceOf(claim).some((e) => e.id === wb.highlight?.evidenceId);
}

function hasLine(claim: Claim) {
  return evidenceOf(claim).some((e) => e.line && e.line.moves.length);
}

function badge(claim: Claim): { text: string; cls: string } | null {
  const kinds = evidenceOf(claim).map((e) => e.certainty);
  if (kinds.includes("tentative")) return { text: "位置判断", cls: "tentative" };
  if (kinds.includes("supported")) return { text: "变化支持", cls: "supported" };
  return null;
}

function open(claim: Claim) {
  const ev = evidenceOf(claim).find((e) => e.line) ?? primary(claim);
  if (ev) showEvidence(ev);
}
</script>

<template>
  <ul class="claims">
    <li v-for="(c, i) in claims" :key="i" :class="{ active: active(c), clickable: !!primary(c) }" @click="open(c)">
      <span class="text">{{ c.text }}</span>
      <span v-if="badge(c)" class="badge" :class="badge(c)!.cls">{{ badge(c)!.text }}</span>
      <span v-if="hasLine(c)" class="replay">▶ 演示</span>
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
  gap: 4px;
}
li {
  padding: 6px 8px;
  border-radius: 6px;
  line-height: 1.6;
  font-size: 14px;
  border: 1px solid transparent;
}
li.clickable {
  cursor: pointer;
}
li.clickable:hover {
  background: var(--hover-bg);
}
li.active {
  border-color: var(--accent-amber);
  background: var(--highlight-bg);
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
.replay {
  margin-left: 6px;
  font-size: 12px;
  color: var(--accent-blue);
  white-space: nowrap;
}
</style>
