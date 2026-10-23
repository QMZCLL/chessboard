<script setup lang="ts">
// 右侧讲解面板：本步讲解（结论、需求、依据、候选比较、主要应对、学习点）、局面分析与笔记。
import { computed, ref, watch } from "vue";
import { NAlert, NButton, NButtonGroup, NInput, NProgress, NScrollbar, NSpin, NSwitch, NTabPane, NTabs, NTag } from "naive-ui";
import AttemptCard from "./AttemptCard.vue";
import CandidateTable from "./CandidateTable.vue";
import ClaimList from "./ClaimList.vue";
import VerdictTag from "./VerdictTag.vue";
import {
  analyze,
  cancelAnalysis,
  cloudLookup,
  explainDeeper,
  newestRecord,
  nodeOf,
  retryExplanation,
  reveal,
  saveNote,
  selectedNode,
  thinking,
  wb,
} from "../state/workbench";
import { sideLabel } from "../utils/board";

const tab = ref<"move" | "position" | "notes">("move");
const node = selectedNode;
const record = computed(() =>
  newestRecord(wb.detail?.node.id === wb.selected ? wb.detail.explanation : null, wb.records[wb.selected]),
);
const expl = computed(() => record.value?.explanation ?? null);
const isMove = computed(() => !!node.value?.mv);
const attempt = computed(() => (node.value ? wb.attempts[node.value.id] : undefined));
const analysisHere = computed(() => (wb.analysis && wb.analysis.nodeId === wb.selected ? wb.analysis : null));
const position = computed(() => analysisHere.value?.position ?? (wb.detail?.node.id === wb.selected ? wb.detail.position : null));
const positionCandidates = computed(() => analysisHere.value?.candidates.length ? analysisHere.value.candidates : position.value?.candidates ?? []);

watch(
  () => wb.selected,
  () => {
    if (!isMove.value && tab.value === "move") tab.value = "position";
    if (isMove.value && tab.value === "position" && !wb.analysis) tab.value = "move";
    note.value = node.value?.note ?? "";
    starred.value = node.value?.starred ?? false;
  },
);

const note = ref(node.value?.note ?? "");
const starred = ref(node.value?.starred ?? false);
async function persistNote() {
  if (node.value && (note.value !== node.value.note || starred.value !== node.value.starred)) {
    await saveNote(note.value, starred.value);
  }
}

const moverLabel = computed(() => (expl.value ? sideLabel(expl.value.mover) : ""));
const LEVEL = {
  basic: { text: "基本讲解", type: "info" },
  partial: { text: "部分讲解", type: "warning" },
  detailed: { text: "详细讲解", type: "success" },
} as const;

// 该着的走棋方就是落子前局面的行棋方
const thinkSide = computed(() => {
  const parent = nodeOf(node.value?.parent);
  return parent ? sideLabel(parent.side_to_move) : "";
});
</script>

<template>
  <section class="panel">
    <n-tabs v-model:value="tab" type="line" size="small" class="tabs" pane-class="pane">
      <n-tab-pane name="move" tab="本步讲解" :disabled="!isMove">
        <n-scrollbar class="scroll">
          <div class="content">
            <!-- 先想后讲 -->
            <div v-if="thinking" class="think">
              <h3>先想一想</h3>
              <p>轮到<strong>{{ thinkSide }}</strong>走。你会怎么走？</p>
              <p class="muted">在棋盘上走出你的选择：与实际着法不同时会建立分支，并与实际着法对照讲解。</p>
              <n-button size="small" @click="reveal()">直接看讲解</n-button>
            </div>
            <template v-else-if="isMove">
              <AttemptCard v-if="attempt !== undefined" :node-id="attempt" />
              <div v-if="wb.detailError" class="error">{{ wb.detailError }}</div>
              <template v-if="expl">
                <div class="headline">
                  <n-tag size="small" :bordered="false" :type="expl.mover === 'red' ? 'error' : 'default'">{{ moverLabel }}</n-tag>
                  <VerdictTag v-if="expl.verdict" :verdict="expl.verdict" />
                  <n-tag size="small" :bordered="false" :type="LEVEL[expl.level].type">
                    {{ expl.level === "detailed" && record?.budget === "deep" ? "深入分析" : LEVEL[expl.level].text }}
                  </n-tag>
                </div>
                <p class="summary">{{ expl.summary }}</p>
                <div v-if="record?.pending" class="pending"><n-spin size="small" /> 正在用引擎比较候选与主要应对，完成后自动补充…</div>
                <n-alert v-if="!record?.pending && (record?.error || expl.level === 'partial')" type="warning" :show-icon="false" class="alert">
                  讲解尚不完整{{ record?.error ? `：${record.error}` : "，下面的提示说明了缺少的部分" }}
                  <n-button size="tiny" quaternary @click="retryExplanation">重试</n-button>
                </n-alert>

                <h4>当前需求</h4>
                <ClaimList :claims="expl.need" :evidence="expl.evidence" />
                <h4>具体依据</h4>
                <ClaimList :claims="expl.basis" :evidence="expl.evidence" />
                <h4>候选比较</h4>
                <ClaimList v-if="expl.comparison.length" :claims="expl.comparison" :evidence="expl.evidence" />
                <p v-else class="muted">等待引擎分析后补充候选比较。</p>
                <CandidateTable v-if="expl.candidates.length" :candidates="expl.candidates" base="parent" show-actual />
                <h4>主要应对</h4>
                <ClaimList v-if="expl.replies.length" :claims="expl.replies" :evidence="expl.evidence" />
                <p v-else class="muted">{{ expl.level === "detailed" ? "引擎没有给出后续变化。" : "等待引擎分析后补充对手的主要应对。" }}</p>
                <h4>学习点</h4>
                <p class="lesson">{{ expl.lesson }}</p>

                <n-alert v-for="w in expl.warnings" :key="w" type="info" :show-icon="false" class="alert">{{ w }}</n-alert>
                <p v-if="expl.conditions" class="conditions">分析条件：{{ expl.conditions }}</p>
                <div class="actions">
                  <n-button size="small" :disabled="record?.pending" @click="explainDeeper">深入分析这一着</n-button>
                </div>
              </template>
              <div v-else-if="!wb.detailError" class="loading"><n-spin size="small" /> 正在生成讲解…</div>
            </template>
          </div>
        </n-scrollbar>
      </n-tab-pane>

      <n-tab-pane name="position" tab="局面分析">
        <n-scrollbar class="scroll">
          <div v-if="position" class="content">
            <p class="summary">{{ position.summary }}</p>
            <ClaimList :claims="position.claims" :evidence="position.evidence" />
            <n-alert v-for="w in position.warnings" :key="w" type="info" :show-icon="false" class="alert">{{ w }}</n-alert>
            <template v-if="position.status === 'ongoing'">
              <h4>引擎候选</h4>
              <div class="analysis-bar">
                <n-button-group size="small">
                  <n-button :loading="analysisHere?.running && analysisHere.budget === 'quick'" @click="analyze('quick')">快速分析</n-button>
                  <n-button :loading="analysisHere?.running && analysisHere.budget === 'deep'" @click="analyze('deep')">深入分析</n-button>
                </n-button-group>
                <n-button v-if="analysisHere?.running" size="small" quaternary @click="cancelAnalysis">取消</n-button>
                <span v-if="analysisHere?.running" class="muted">深度 {{ analysisHere.depth }}</span>
              </div>
              <n-progress v-if="analysisHere?.running" type="line" :percentage="Math.min(99, analysisHere.depth * 4)" :show-indicator="false" processing />
              <p v-if="analysisHere?.error" class="error">{{ analysisHere.error }}</p>
              <CandidateTable :candidates="positionCandidates" base="node" />
              <p v-if="position.conditions" class="conditions">分析条件：{{ position.conditions }}</p>
              <div v-if="wb.status?.config.cloud.enabled" class="cloud">
                <n-button size="small" quaternary @click="cloudLookup">查询云库</n-button>
                <span v-if="wb.detail?.cloud" class="muted">
                  {{ wb.detail.cloud.source }}：
                  <template v-if="wb.detail.cloud.outcome.kind === 'moves'">
                    评分 {{ wb.detail.cloud.outcome.score }} · {{ wb.detail.cloud.pv_notation.slice(0, 4).join(" ") }}
                  </template>
                  <template v-else-if="wb.detail.cloud.outcome.kind === 'unknown'">没有该局面的数据</template>
                  <template v-else-if="wb.detail.cloud.outcome.kind === 'invalid_board'">认为局面不成立，请检查棋子与行棋方</template>
                  <template v-else>认为对局已结束</template>
                  （{{ new Date(wb.detail.cloud.queried_at).toLocaleString() }}）
                </span>
              </div>
            </template>
          </div>
          <div v-else class="loading"><n-spin size="small" /></div>
        </n-scrollbar>
      </n-tab-pane>

      <n-tab-pane name="notes" tab="笔记">
        <div class="content notes">
          <div class="note-head">
            <span>收藏这一步</span>
            <n-switch v-model:value="starred" size="small" @update:value="persistNote" />
          </div>
          <n-input
            v-model:value="note"
            type="textarea"
            :autosize="{ minRows: 6, maxRows: 16 }"
            placeholder="记录你的想法、疑问或要复习的要点"
            @blur="persistNote"
          />
          <p class="muted">离开输入框时自动保存。</p>
        </div>
      </n-tab-pane>
    </n-tabs>
  </section>
</template>

<style scoped>
.panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.tabs {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.tabs :deep(.n-tabs-nav) {
  padding: 0 12px;
}
.tabs :deep(.pane) {
  flex: 1;
  min-height: 0;
  padding: 0;
}
.scroll {
  height: 100%;
}
.content {
  padding: 8px 14px 24px;
}
.headline {
  display: flex;
  gap: 6px;
  align-items: center;
  flex-wrap: wrap;
}
.summary {
  font-size: 17px;
  line-height: 1.7;
  margin: 8px 0;
  font-weight: 600;
}
h4 {
  margin: 14px 0 4px;
  font-size: 13px;
  color: var(--text-muted);
  font-weight: 600;
  letter-spacing: 1px;
}
.lesson {
  margin: 0;
  padding: 8px 10px;
  border-left: 3px solid var(--accent-green);
  background: var(--tag-green-bg);
  border-radius: 0 6px 6px 0;
  line-height: 1.7;
}
.pending {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--accent-blue);
}
.alert {
  margin: 8px 0;
  font-size: 13px;
}
.conditions {
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 12px;
}
.actions {
  margin-top: 10px;
}
.muted {
  color: var(--text-muted);
  font-size: 13px;
}
.error {
  color: var(--accent-red);
  font-size: 13px;
}
.loading {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 16px;
  color: var(--text-muted);
}
.think {
  padding: 12px;
  border: 1px dashed var(--accent-blue);
  border-radius: 8px;
}
.think h3 {
  margin: 0 0 6px;
}
.analysis-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}
.cloud {
  margin-top: 10px;
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.notes {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.note-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
</style>
