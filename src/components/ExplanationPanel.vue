<script setup lang="ts">
// 右侧讲解面板。本步讲解按阅读顺序组织：当前着法与程序评价 → 一句话要点 → 讲解正文
// （有 AI 讲解时用 AI 讲解，否则用规则讲解）→ 关键操作 → 详细分析（可折叠）→ 学习点与笔记。
// 局面分析在起始局面上直接给出分析入口；笔记显示保存状态，并始终保存到编辑时对应的那一着。
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { NAlert, NButton, NCollapse, NCollapseItem, NInput, NScrollbar, NSpin, NSwitch, NTabPane, NTabs, NTag, NTooltip, useMessage } from "naive-ui";
import AttemptCard from "./AttemptCard.vue";
import BasisList from "./BasisList.vue";
import Icon from "./Icon.vue";
import CandidateTable from "./CandidateTable.vue";
import CoachCard from "./CoachCard.vue";
import ComparePanel from "./ComparePanel.vue";
import ClaimList from "./ClaimList.vue";
import VerdictTag from "./VerdictTag.vue";
import {
  analyze,
  canDemo,
  cancelAnalysis,
  cloudLookup,
  coachView,
  demoEvidence,
  exitPreview,
  explainDeeper,
  newestRecord,
  nodeOf,
  previewBackLabel,
  retryExplanation,
  reveal,
  selectedNode,
  showHint,
  thinkHints,
  toggleHintMove,
  concealed,
  study,
  thinking,
  wb,
} from "../state/workbench";
import { coachLesson } from "../state/coachContent";
import { cmp } from "../state/compare";
import { coachDisplay } from "../state/coachDisplay";
import { editNote, noteView, persistNote } from "../state/notes";
import { addToReview, openReviews } from "../state/review";
import { reviewCardId, STATUS_LABEL } from "../state/reviewList";
import { errorMessage } from "../api/backend";
import { sideLabel } from "../utils/board";

const tab = ref<"move" | "position" | "notes" | "compare">("move");
const node = selectedNode;
const record = computed(() =>
  newestRecord(wb.detail?.node.id === wb.selected ? wb.detail.explanation : null, wb.records[wb.selected]),
);
const expl = computed(() => record.value?.explanation ?? null);
const isMove = computed(() => !!node.value?.mv);
const attempt = computed(() => (node.value ? wb.attempts[node.value.id] : undefined));
const analysisHere = computed(() => (wb.analysis && wb.analysis.nodeId === wb.selected ? wb.analysis : null));
const position = computed(() => analysisHere.value?.position ?? (wb.detail?.node.id === wb.selected ? wb.detail.position : null));
const positionCandidates = computed(() => (analysisHere.value?.candidates.length ? analysisHere.value.candidates : position.value?.candidates ?? []));

/** 一句话要点：模板讲解的结论去掉开头的着法名（着法已在标题中）。 */
const keyPoint = computed(() => {
  const e = expl.value;
  if (!e) return "";
  const prefix = `${e.notation}：`;
  return e.summary.startsWith(prefix) ? e.summary.slice(prefix.length) : e.summary;
});
/** AI 讲解的结果对应当前条件时才承担正文，规则讲解的依据移到详细分析中；上次的结果不作为正文。 */
const coachBody = computed(() => coachDisplay(coachView.value).kind === "current");
const coachResult = computed(() => (coachBody.value ? (coachView.value?.record?.result ?? null) : null));
/** 当前有效的 AI 学习要点；没有时学习点使用规则讲解的内容 */
const aiLesson = computed(() => coachLesson(coachResult.value));
/** 讲解不完整（部分分析或出错）时，补充分析是主要操作 */
const needsRetry = computed(() => !record.value?.pending && (!!record.value?.error || expl.value?.level === "partial"));
const replyEvidence = computed(() => expl.value?.evidence.find((e) => e.topic === "reply" && canDemo(e)) ?? null);

// 详细分析的展开状态在本机记住
const DETAIL_KEY = "xq-detail-open";
function storedOpen(): string[] {
  try {
    return localStorage.getItem(DETAIL_KEY) === "1" ? ["detail"] : [];
  } catch {
    return [];
  }
}
const detailOpen = ref<string[]>(storedOpen());
function setDetailOpen(names: Array<string | number>) {
  detailOpen.value = names.map(String);
  try {
    localStorage.setItem(DETAIL_KEY, detailOpen.value.length ? "1" : "0");
  } catch {
    // 记不住展开状态不影响使用
  }
}
const candidatesAnchor = ref<HTMLElement | null>(null);
function showCandidates() {
  setDetailOpen(["detail"]);
  setTimeout(() => candidatesAnchor.value?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
}

watch(
  () => wb.selected,
  () => {
    if (!isMove.value && tab.value === "move") tab.value = "position";
    if (isMove.value && tab.value === "position" && !wb.analysis) tab.value = "move";
  },
);
// 先想后讲时，思考提示在本步讲解中，切到其他标签会看不到
watch(thinking, (t) => {
  if (t) tab.value = "move";
});
// 打开对比时切到对比；关闭后回到本步讲解或局面分析
watch(
  () => cmp.request && `${cmp.request.origin}:${cmp.request.mine}`,
  (key, before) => {
    if (key && key !== before) tab.value = "compare";
    else if (!key && tab.value === "compare") tab.value = isMove.value ? "move" : "position";
  },
);

// ---------- 笔记 ----------
// 草稿与保存状态在 state/notes.ts 中按（会话, 着法）保存，面板重建（如跨布局断点）后不丢失也不回写旧值
const noteKey = computed(() => (study.value ? { studyId: study.value.id, nodeId: wb.selected } : null));
const noteShown = computed(() => (noteKey.value ? noteView(noteKey.value.studyId, noteKey.value.nodeId) : null));

function onNoteText(text: string) {
  if (noteKey.value) editNote(noteKey.value.studyId, noteKey.value.nodeId, { text });
}

function onStarred(starred: boolean) {
  if (!noteKey.value) return;
  editNote(noteKey.value.studyId, noteKey.value.nodeId, { starred });
  void persistNote(noteKey.value.studyId, noteKey.value.nodeId);
}

function saveCurrentNote() {
  if (noteKey.value) void persistNote(noteKey.value.studyId, noteKey.value.nodeId);
}

// 面板被移除（例如窗口跨过布局断点）时，保存正在编辑的内容；草稿本身仍在共享状态中
onBeforeUnmount(saveCurrentNote);

const moverLabel = computed(() => (expl.value ? sideLabel(expl.value.mover) : ""));
const LEVEL = {
  basic: { text: "规则讲解", type: "info" },
  partial: { text: "分析不完整", type: "warning" },
  detailed: { text: "引擎分析完整", type: "success" },
} as const;

// 该着的走棋方就是落子前局面的行棋方
const thinkSide = computed(() => {
  const parent = nodeOf(node.value?.parent);
  return parent ? sideLabel(parent.side_to_move) : "";
});
const isRoot = computed(() => node.value?.id === 0);

// ---------- 先想后讲 ----------
/** 正在思考的这一着尚未揭示：隐藏会透露答案的标签 */
const hiding = computed(() => concealed.value.has(wb.selected));
const hintLevel = computed(() => (wb.thinkSession?.nodeId === wb.selected ? wb.thinkSession.hints : 0));
/** 这一着最近一次练习的记录 */
const practiceText = computed(() => {
  const p = node.value?.practice;
  if (!p) return "";
  const hints = p.hints_used ? `用了 ${p.hints_used} 级提示` : "没有用提示";
  const choice = p.revealed_directly
    ? "没有走子就看了讲解"
    : `第一步${p.matched ? "与棋谱相同" : "与棋谱不同"}（${notationOfChoice(p.first_choice)}）`;
  return `练习记录：${hints}，${choice}${p.sessions > 1 ? ` · 共练习 ${p.sessions} 次` : ""}`;
});
function notationOfChoice(mv: string | null) {
  if (!mv) return "";
  const parent = nodeOf(node.value?.parent);
  const child = parent?.children.map((c) => nodeOf(c)).find((c) => c?.mv === mv);
  return child?.notation ?? mv;
}
const budgetLabel = (b: string) => (b === "deep" ? "深入分析" : "快速分析");

// ---------- 复习 ----------
const message = useMessage();
/** 这一着在复习中时的状态（来自节点详情） */
const reviewStatus = computed(() => (wb.detail?.node.id === wb.selected ? wb.detail.review : null));
async function addReview() {
  const s = study.value;
  if (!s) return;
  try {
    await addToReview(s.id, wb.selected);
    message.success("已加入复习：在顶部的“复习”中先看局面重新选择，再对照这次的思考与讲解");
  } catch (e) {
    message.error(errorMessage(e));
  }
}
function openThisReview() {
  if (study.value) openReviews(reviewCardId(study.value.id, wb.selected));
}
</script>

<template>
  <section class="panel">
    <n-tabs v-model:value="tab" type="line" size="small" class="tabs" pane-class="pane">
      <n-tab-pane name="move" tab="本步讲解" :disabled="!isMove">
        <n-scrollbar class="scroll">
          <div class="content">
            <!-- 先想后讲 -->
            <div v-if="thinking" class="think" data-testid="think-card">
              <h3>先想一想</h3>
              <p>轮到<strong>{{ thinkSide }}</strong>走。你会怎么走？</p>
              <p class="muted">在棋盘上走出你的选择：与实际着法不同时会建立分支，并与实际着法对照讲解。</p>
              <!-- 分级提示：只依据当前局面，不透露棋谱中的着法 -->
              <ol v-if="hintLevel > 0 && thinkHints" class="hints" data-testid="hints">
                <li class="hint">
                  <span class="hint-label">提示 1 · 局面需求</span>
                  <p>{{ thinkHints.observe }}</p>
                </li>
                <li v-if="hintLevel >= 2" class="hint">
                  <span class="hint-label">提示 2 · 相关棋子</span>
                  <p>{{ thinkHints.focus.text }}</p>
                  <p v-if="thinkHints.focus.squares.length" class="muted">已在棋盘上标出。</p>
                </li>
                <li v-if="hintLevel >= 3" class="hint">
                  <span class="hint-label">提示 3 · 可以考虑</span>
                  <div v-if="thinkHints.moves.length" class="hint-moves">
                    <button
                      v-for="m in thinkHints.moves"
                      :key="m.mv"
                      type="button"
                      class="hint-move focusable"
                      :class="{ on: wb.thinkSession?.arrow === m.mv }"
                      :aria-pressed="wb.thinkSession?.arrow === m.mv"
                      @click="toggleHintMove(m.mv)"
                    >
                      {{ m.notation }}
                    </button>
                  </div>
                  <p class="muted">{{ thinkHints.moves_note }}{{ thinkHints.moves.length ? "。点一步可在棋盘上查看，再自己走出来" : "" }}</p>
                </li>
              </ol>
              <div class="think-actions">
                <n-button v-if="thinkHints && hintLevel < 3" size="medium" type="primary" data-testid="hint-next" @click="showHint">
                  {{ hintLevel === 0 ? "给我一点提示" : "再给一点提示" }}
                </n-button>
                <n-button size="medium" secondary data-testid="reveal" @click="reveal()">直接看讲解</n-button>
                <span v-if="thinkHints" class="muted" data-testid="hint-count">已用提示 {{ hintLevel }} / 3</span>
              </div>
              <p class="muted note">提示只根据当前局面的事实与候选生成，不会直接告诉你棋谱中的着法。</p>
            </div>
            <template v-else-if="isMove">
              <!-- 先想后讲揭示之后：练习记录旁就是“加入复习”，保留这次的思考 -->
              <div v-if="practiceText" class="practice">
                <span data-testid="practice">{{ practiceText }}</span>
                <n-button v-if="!reviewStatus" size="tiny" secondary data-testid="add-review" @click="addReview">加入复习</n-button>
                <n-button v-else size="tiny" quaternary data-testid="in-review" @click="openThisReview">复习中 · {{ STATUS_LABEL[reviewStatus] }}</n-button>
              </div>
              <AttemptCard v-if="attempt !== undefined" :node-id="attempt" />
              <div v-if="wb.detailError" class="error">{{ wb.detailError }}</div>
              <template v-if="expl">
                <!-- 棋盘正在演示其他局面时，说明右侧讲解的对象 -->
                <div v-if="wb.preview" class="subject-bar" data-testid="explain-subject">
                  <span>
                    本页讲解：<strong class="subject-move">{{ expl.notation }}</strong>
                    <span class="muted"> · 棋盘正在{{ wb.preview.moves.length ? "演示变化" : "显示其他局面" }}</span>
                  </span>
                  <n-button size="small" secondary @click="exitPreview">
                    <template #icon><Icon name="back" /></template>{{ previewBackLabel }}
                  </n-button>
                </div>
                <!-- 1. 当前着法与程序评价 -->
                <header class="move-head">
                  <span class="mover" :class="expl.mover">{{ moverLabel }}</span>
                  <h2 class="move-name" data-testid="move-name">{{ expl.notation }}</h2>
                  <div class="head-tags">
                    <VerdictTag v-if="expl.verdict" :verdict="expl.verdict" />
                    <n-tag size="small" :bordered="false" :type="LEVEL[expl.level].type">
                      {{ expl.level === "detailed" && record?.budget === "deep" ? "深入分析完整" : LEVEL[expl.level].text }}
                    </n-tag>
                  </div>
                </header>
                <!-- 2. 一句话要点 -->
                <p class="key-point" data-testid="key-point">{{ keyPoint }}</p>
                <div v-if="record?.pending" class="status-line"><n-spin :size="14" /> 引擎正在比较候选与主要应对，完成后自动补充</div>
                <!-- 分析不完整时，补充分析是这一区域的主要操作；已确认的事实照常显示 -->
                <n-alert v-if="needsRetry" type="warning" :show-icon="false" class="alert" data-testid="partial-alert">
                  <div class="alert-row">
                    <span>{{ record?.error ? `讲解尚不完整：${record.error}` : "分析不完整：只显示已确认的事实，详细分析中说明了缺少的部分。" }}</span>
                    <n-button size="small" type="warning" @click="retryExplanation">{{ record?.error ? "重试" : "补充分析" }}</n-button>
                  </div>
                </n-alert>

                <!-- 3. 讲解正文 -->
                <CoachCard v-if="coachBody" :level="expl.level" :pending="!!record?.pending" />
                <template v-else>
                  <h3 class="group section-title">为什么这样走</h3>
                  <ClaimList scope="basis" :claims="expl.basis" :evidence="expl.evidence" />
                  <CoachCard :level="expl.level" :pending="!!record?.pending" />
                </template>

                <!-- 4. 关键操作 -->
                <div class="key-actions">
                  <n-button v-if="replyEvidence" size="medium" :type="needsRetry ? 'default' : 'primary'" secondary data-testid="demo-reply" @click="demoEvidence(replyEvidence)">
                    <template #icon><Icon name="play" :size="13" /></template>演示主要应对
                  </n-button>
                  <n-button v-if="expl.candidates.length" size="medium" secondary @click="showCandidates">
                    <template #icon><Icon name="list" /></template>查看候选
                  </n-button>
                  <n-tooltip>
                    <template #trigger>
                      <n-button size="medium" quaternary :disabled="record?.pending" @click="explainDeeper">
                        <template #icon><Icon name="search" /></template>深入分析
                      </n-button>
                    </template>
                    用更长的时间重新分析这一着
                  </n-tooltip>
                </div>

                <!-- 5. 详细分析 -->
                <n-collapse :expanded-names="detailOpen" class="detail" @update:expanded-names="setDetailOpen">
                  <n-collapse-item title="详细分析" name="detail">
                    <h4 class="section-title">当前需求</h4>
                    <ClaimList scope="need" :claims="expl.need" :evidence="expl.evidence" />
                    <h4 ref="candidatesAnchor" class="section-title">候选比较</h4>
                    <ClaimList v-if="expl.comparison.length" scope="comparison" :claims="expl.comparison" :evidence="expl.evidence" />
                    <p v-else class="muted">等待引擎分析后补充候选比较。</p>
                    <CandidateTable v-if="expl.candidates.length" scope="move-cand" :candidates="expl.candidates" base="parent" show-actual />
                    <h4 class="section-title">主要应对</h4>
                    <ClaimList v-if="expl.replies.length" scope="replies" :claims="expl.replies" :evidence="expl.evidence" />
                    <p v-else class="muted">{{ expl.level === "detailed" ? "引擎没有给出后续变化。" : "等待引擎分析后补充对手的主要应对。" }}</p>
                    <!-- 有效 AI 讲解出现后移到这里的规则讲解内容放在末尾：结果到达时不把正在阅读的段落往下推 -->
                    <template v-if="coachBody">
                      <h4 class="section-title">规则依据</h4>
                      <ClaimList scope="basis" :claims="expl.basis" :evidence="expl.evidence" />
                    </template>
                    <template v-if="aiLesson">
                      <h4 class="section-title">规则讲解的学习点</h4>
                      <p class="rule-lesson">{{ expl.lesson }}</p>
                    </template>
                    <n-alert v-for="w in expl.warnings" :key="w" type="info" :show-icon="false" class="alert">{{ w }}</n-alert>
                    <p v-if="expl.conditions" class="conditions">分析条件：{{ expl.conditions }}</p>
                  </n-collapse-item>
                </n-collapse>

                <!-- 6. 学习点与笔记 -->
                <!-- 学习点只有一套：有当前有效的 AI 学习要点时用它（带程序依据），否则用规则讲解的学习点 -->
                <div class="lesson" data-testid="lesson">
                  <span class="lesson-title">学习点<template v-if="aiLesson"> · AI 讲解</template></span>
                  <template v-if="aiLesson && coachResult">
                    <ClaimList scope="ai-lesson" :claims="[aiLesson.claim]" :evidence="coachResult.evidence" class="lesson-claim" />
                    <BasisList :items="aiLesson.basis" />
                  </template>
                  <p v-else>{{ expl.lesson }}</p>
                </div>
                <div class="record-actions">
                  <n-button text size="small" @click="tab = 'notes'">
                    <template #icon><Icon name="pen" :size="14" /></template>{{ node?.note ? "查看这一步的笔记" : "为这一步写笔记" }}
                  </n-button>
                  <template v-if="!practiceText">
                    <n-button v-if="!reviewStatus" text size="small" data-testid="add-review" @click="addReview">
                      <template #icon><Icon name="book" :size="14" /></template>加入复习
                    </n-button>
                    <n-button v-else text size="small" data-testid="in-review" @click="openThisReview">
                      <template #icon><Icon name="book" :size="14" /></template>复习中 · {{ STATUS_LABEL[reviewStatus] }}
                    </n-button>
                  </template>
                </div>
              </template>
              <div v-else-if="!wb.detailError" class="loading"><n-spin size="small" /> 正在生成讲解…</div>
            </template>
          </div>
        </n-scrollbar>
      </n-tab-pane>

      <!-- 先想后讲时，局面分析显示的是落子后的局面、笔记可能写着答案，揭示前不可用 -->
      <n-tab-pane name="position" tab="局面分析" :disabled="hiding">
        <n-scrollbar class="scroll">
          <div v-if="position" class="content">
            <div v-if="isRoot && position.status === 'ongoing'" class="intro" data-testid="start-intro">
              当前是起始局面。可以先分析局面，或在棋盘上走出一步开始学习。
            </div>
            <p class="position-summary">{{ position.summary }}</p>
            <template v-if="position.status === 'ongoing'">
              <div class="analysis-bar">
                <n-button
                  :type="analysisHere?.position || analysisHere?.running ? 'default' : 'primary'"
                  size="small"
                  :loading="analysisHere?.running && analysisHere.budget === 'quick'"
                  data-testid="analyze-position"
                  @click="analyze('quick')"
                >
                  分析这个局面
                </n-button>
                <n-button size="small" :loading="analysisHere?.running && analysisHere.budget === 'deep'" @click="analyze('deep')">深入分析</n-button>
                <n-button v-if="analysisHere?.running" size="small" quaternary @click="cancelAnalysis">取消</n-button>
              </div>
              <div v-if="analysisHere?.running" class="progress" role="status">
                <div class="indeterminate" />
                <span>正在{{ budgetLabel(analysisHere.budget) }} · 当前搜索深度 {{ analysisHere.depth }}</span>
              </div>
              <p v-if="analysisHere?.error" class="error">{{ analysisHere.error }}</p>
            </template>
            <ClaimList scope="pos" :claims="position.claims" :evidence="position.evidence" />
            <n-alert v-for="w in position.warnings" :key="w" type="info" :show-icon="false" class="alert">{{ w }}</n-alert>
            <template v-if="position.status === 'ongoing'">
              <h4 class="section-title">引擎候选</h4>
              <CandidateTable scope="pos-cand" :candidates="positionCandidates" base="node" />
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

      <n-tab-pane v-if="cmp.request" name="compare" tab="对比">
        <n-scrollbar class="scroll">
          <ComparePanel />
        </n-scrollbar>
      </n-tab-pane>

      <n-tab-pane name="notes" tab="笔记" :disabled="hiding">
        <div class="content notes">
          <div class="note-head">
            <span>收藏这一步</span>
            <n-switch :value="noteShown?.starred ?? false" size="small" @update:value="onStarred" />
          </div>
          <n-input
            :value="noteShown?.text ?? ''"
            type="textarea"
            :autosize="{ minRows: 6, maxRows: 16 }"
            placeholder="记录你的想法、疑问或要复习的要点"
            data-testid="note-input"
            @update:value="onNoteText"
            @blur="saveCurrentNote"
          />
          <p class="muted" data-testid="note-state">
            <template v-if="noteShown?.state === 'saving'">保存中…</template>
            <template v-else-if="noteShown?.state === 'saved'">已保存</template>
            <span v-else-if="noteShown?.state === 'failed'" class="error">保存失败：{{ noteShown.error }}（离开输入框时会再次保存）</span>
            <template v-else-if="noteShown?.state === 'editing'">有未保存的修改，离开输入框时保存。</template>
            <template v-else>离开输入框时自动保存。</template>
          </p>
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
  padding: var(--sp-2) var(--sp-4) 0;
}
.tabs :deep(.n-tabs-tab) {
  padding-bottom: 8px;
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
  padding: var(--sp-4) var(--sp-4) var(--sp-5);
  font-size: var(--fs-body);
  line-height: var(--lh-body);
}
.move-head {
  display: flex;
  gap: var(--sp-2);
  align-items: center;
  flex-wrap: wrap;
}
.mover {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  font-weight: 600;
  padding: 1px 9px 1px 7px;
  border-radius: 999px;
  background: var(--tag-gray-bg);
}
/* 评价与分析程度作为一组，空间不够时一起换行 */
.head-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.mover::before {
  content: "";
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
}
.mover.red {
  background: var(--tag-red-bg);
  color: var(--accent-red);
}
/* 棋盘显示的不是本页讲解的局面时，说明讲解对象并提供返回 */
.subject-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin: 0 0 var(--sp-3);
  padding: 6px 6px 6px var(--sp-3);
  border-radius: var(--radius);
  background: var(--tag-blue-bg);
  font-size: var(--fs-group);
}
.subject-move {
  font-family: var(--piece-font);
  font-size: 16px;
  letter-spacing: 1px;
}
.alert-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.rule-lesson {
  margin: 0;
  padding-left: var(--sp-3);
}
.lesson-claim :deep(.claim) {
  padding-left: 0;
  font-family: var(--font-serif);
}
.lesson-claim :deep(.claim::before) {
  display: none;
}
.lesson :deep(.basis) {
  margin-left: 0;
  background: transparent;
}
.move-name {
  margin: 0 2px 0 0;
  font-family: var(--piece-font);
  font-size: var(--fs-move);
  font-weight: 700;
  letter-spacing: 2px;
  line-height: 1.3;
}
/* 一句话要点：宋体，作为这一着的“标题句” */
.key-point {
  font-family: var(--font-serif);
  font-size: var(--fs-key);
  line-height: 1.6;
  margin: var(--sp-3) 0 var(--sp-2);
  font-weight: 700;
  color: var(--text-primary);
}
.position-summary {
  font-family: var(--font-serif);
  font-weight: 700;
  font-size: 16px;
  margin: var(--sp-2) 0 var(--sp-3);
}
h3.group,
h4 {
  margin: var(--sp-4) 0 var(--sp-2);
}
.status-line {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  font-size: var(--fs-aux);
  color: var(--accent-blue);
  margin-bottom: var(--sp-2);
}
.alert {
  margin: var(--sp-2) 0;
  font-size: var(--fs-aux);
}
.key-actions :deep(.n-button) {
  padding: 0 12px;
}
.key-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: var(--sp-3) 0 var(--sp-2);
}
.detail {
  margin: var(--sp-2) 0 var(--sp-3);
}
.detail {
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
  padding: 2px 0;
}
.detail :deep(.n-collapse-item__header) {
  padding: 8px 0;
}
.detail :deep(.n-collapse-item__header-main) {
  font-weight: 600;
  font-size: var(--fs-group);
  color: var(--text-secondary);
}
/* 学习点：一张带色条的小卡片 */
.lesson {
  position: relative;
  margin-top: var(--sp-3);
  padding: var(--sp-3) var(--sp-4) var(--sp-3) calc(var(--sp-4) + 4px);
  background: var(--tag-green-bg);
  border-radius: var(--radius);
  overflow: hidden;
}
.lesson::before {
  content: "";
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 4px;
  background: var(--accent-green);
}
.lesson-title {
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.1em;
  color: var(--accent-green);
}
.lesson p {
  margin: 4px 0 0;
  font-family: var(--font-serif);
}
.record-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-1) var(--sp-4);
  margin-top: var(--sp-2);
}
.conditions {
  font-size: var(--fs-meta);
  color: var(--text-muted);
  margin-top: var(--sp-3);
}
.muted {
  color: var(--text-muted);
  font-size: var(--fs-aux);
}
.error {
  color: var(--accent-red);
  font-size: var(--fs-aux);
}
.loading {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-4);
  color: var(--text-muted);
}
.think {
  padding: var(--sp-4);
  border-radius: var(--radius-lg);
  background: var(--tag-blue-bg);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent-blue) 30%, transparent);
}
/* 分级提示 */
.hints {
  list-style: none;
  margin: var(--sp-3) 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.hint {
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--radius);
  background: var(--panel);
  box-shadow: inset 0 0 0 1px var(--border);
}
.hint p {
  margin: 2px 0 0;
}
.hint-label {
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.06em;
  color: var(--accent-amber);
}
.hint-moves {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
}
.hint-move {
  min-height: 32px;
  padding: 0 12px;
  border: none;
  border-radius: 999px;
  background: var(--tag-blue-bg);
  color: var(--accent-blue);
  font-family: var(--piece-font);
  font-size: 16px;
  cursor: pointer;
}
.hint-move.on {
  background: var(--accent-blue);
  color: var(--panel);
}
.think-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-top: var(--sp-3);
}
.think .note {
  margin: var(--sp-2) 0 0;
}
.practice {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 6px var(--sp-2);
  margin: 0 0 var(--sp-3);
  padding: 6px var(--sp-3);
  border-radius: var(--radius);
  background: var(--tag-amber-bg);
  font-size: var(--fs-group);
  color: var(--text-secondary);
}
.think h3 {
  margin: 0 0 6px;
  font-family: var(--font-serif);
  font-size: 18px;
}
.intro {
  padding: var(--sp-3) var(--sp-4);
  border-radius: var(--radius);
  background: var(--tag-teal-bg);
  color: var(--text-secondary);
  margin-bottom: var(--sp-2);
}
.analysis-bar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
  margin-bottom: var(--sp-2);
}
.progress {
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
  font-size: var(--fs-aux);
  color: var(--accent-blue);
  margin-bottom: var(--sp-2);
}
/* 不确定进度：只表示正在进行，不把搜索深度换算成百分比 */
.indeterminate {
  height: 3px;
  border-radius: 2px;
  background: linear-gradient(90deg, transparent 0%, var(--accent-blue) 50%, transparent 100%) 0 0 / 40% 100% no-repeat,
    var(--tag-blue-bg);
  animation: slide 1.4s ease-in-out infinite;
}
@keyframes slide {
  from {
    background-position: -40% 0, 0 0;
  }
  to {
    background-position: 140% 0, 0 0;
  }
}
.cloud {
  margin-top: var(--sp-2);
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.notes {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.note-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
</style>
