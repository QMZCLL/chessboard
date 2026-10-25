<script setup lang="ts">
// 复习：左侧是卡片列表，右侧复习一张卡片。先只显示局面，在棋盘上重新选择（或直接看答案）后，
// 再显示这次的结果、原来的思考与看过的提示、棋谱中的这一着与程序讲解，并记下自己的理解或疑问。
// 列表与复习开始前都不显示棋谱中的着法和笔记，避免提前透露答案。
import { computed, onMounted } from "vue";
import { NAlert, NButton, NEmpty, NInput, NModal, NPopconfirm, NRadioButton, NRadioGroup, NTag, NTooltip, useMessage } from "naive-ui";
import XiangqiBoard from "./XiangqiBoard.vue";
import VerdictTag from "./VerdictTag.vue";
import type { ReviewStatus } from "../api/types";
import { errorMessage } from "../api/backend";
import {
  addStarred,
  deleteCard,
  editNote,
  loadReviews,
  nextCardId,
  openSource,
  retryCard,
  review,
  reviewCounts,
  saveNote,
  setStatus,
  shownItems,
  showCard,
  showNext,
  starredNotInReview,
  submitChoice,
} from "../state/review";
import { attemptText, historyText, positionLabel, STATUS_LABEL, suggestedStatus, thinkingText, whenText } from "../state/reviewList";
import { study, wb } from "../state/workbench";
import { sideLabel } from "../utils/board";

const message = useMessage();
const current = computed(() => review.current);
const card = computed(() => current.value?.card ?? null);
const revealed = computed(() => review.phase === "revealed");
const STATUS_TYPE: Record<ReviewStatus, "warning" | "error" | "success"> = { due: "warning", again: "error", understood: "success" };
const LEVEL = { basic: "规则讲解", partial: "分析不完整", detailed: "引擎分析完整" } as const;

// 揭示后：实线是棋谱中的这一着，虚线是这次的选择（与棋谱不同时）
const answerArrows = computed(() => (revealed.value && card.value ? [card.value.answer.mv] : []));
const choiceArrow = computed(() => {
  const a = review.attempt;
  return revealed.value && a?.choice && !a.matched ? a.choice.mv : null;
});
const lastMove = computed(() => card.value?.history[card.value.history.length - 1]?.mv ?? null);
const suggestion = computed(() => (review.attempt ? suggestedStatus(review.attempt.matched) : null));
const hints = computed(() => card.value?.thinking?.hints ?? null);
const recent = computed(() => (card.value?.attempts ?? []).slice(-8));

async function addFromStarred() {
  try {
    const n = await addStarred();
    message.success(`已把 ${n} 着收藏的着法加入复习`);
    if (!review.current && shownItems.value[0]) await showCard(shownItems.value[0].card.id);
  } catch (e) {
    message.error(errorMessage(e));
  }
}

async function open() {
  if (!current.value) return;
  try {
    await openSource(current.value);
  } catch (e) {
    message.error(errorMessage(e));
  }
}

async function mark(status: ReviewStatus) {
  await saveNote();
  await setStatus(status);
  message.success(`已标为“${STATUS_LABEL[status]}”`);
}

onMounted(loadReviews);
</script>

<template>
  <n-modal v-model:show="wb.dialogs.review" preset="card" title="复习" style="width: min(1120px, 96vw)" data-testid="review-dialog" @after-leave="saveNote">
    <div class="review">
      <!-- 卡片列表 -->
      <aside class="list">
        <n-radio-group v-model:value="review.filter" size="small" class="filter">
          <n-radio-button value="todo" data-testid="review-filter-todo">待复习 {{ reviewCounts.todo }}</n-radio-button>
          <n-radio-button value="understood" data-testid="review-filter-understood">已理解 {{ reviewCounts.understood }}</n-radio-button>
          <n-radio-button value="all" data-testid="review-filter-all">全部 {{ reviewCounts.all }}</n-radio-button>
        </n-radio-group>
        <n-button v-if="starredNotInReview.length" size="small" quaternary class="add-starred" data-testid="review-add-starred" @click="addFromStarred">
          把这盘棋收藏的 {{ starredNotInReview.length }} 着加入复习
        </n-button>
        <ol v-if="shownItems.length" class="cards" data-testid="review-list">
          <li v-for="item in shownItems" :key="item.card.id">
            <button
              type="button"
              class="card-item focusable"
              :class="{ active: card?.id === item.card.id }"
              :data-review="item.card.id"
              @click="showCard(item.card.id)"
            >
              <span class="item-top">
                <span class="item-name">{{ item.study_name }}</span>
                <n-tag size="tiny" :type="STATUS_TYPE[item.card.status]" :bordered="false">{{ STATUS_LABEL[item.card.status] }}</n-tag>
              </span>
              <span class="item-sub">{{ positionLabel(item.card) }} · {{ whenText(item.card.reviewed_at) }}</span>
            </button>
          </li>
        </ol>
        <n-empty v-else size="small" class="empty" :description="review.items.length ? '这一组没有卡片' : '还没有复习卡片'">
          <template v-if="!review.items.length" #extra>
            <p class="muted">在讲解中点“加入复习”，或把收藏的着法加入复习。</p>
          </template>
        </n-empty>
      </aside>

      <!-- 复习一张卡片 -->
      <section v-if="card && current" class="practice" data-testid="review-card">
        <header class="head">
          <div>
            <div class="title">{{ current.study_name }} · {{ positionLabel(card) }}</div>
            <div class="muted" data-testid="review-history">{{ historyText(card) }}</div>
          </div>
          <div class="head-actions">
            <n-tag size="small" :type="STATUS_TYPE[card.status]" :bordered="false" data-testid="review-status">{{ STATUS_LABEL[card.status] }}</n-tag>
            <n-tooltip :disabled="current.source_available">
              <template #trigger>
                <n-button size="small" quaternary :disabled="!current.source_available || review.busy" data-testid="review-open-source" @click="open">
                  打开原局面
                </n-button>
              </template>
              原会话中的这一着已经不在（会话或这一着已删除），卡片仍可复习
            </n-tooltip>
            <n-popconfirm @positive-click="deleteCard(card.id)">
              <template #trigger><n-button size="small" quaternary>删除卡片</n-button></template>
              删除这张复习卡片？原会话中的棋谱与笔记不受影响。
            </n-popconfirm>
          </div>
        </header>

        <div class="body">
          <div class="board">
            <XiangqiBoard
              :fen="card.fen"
              :orientation="card.orientation"
              :legal-moves="revealed ? [] : current.legal_moves"
              :last-move="lastMove"
              :arrows="answerArrows"
              :next-arrow="choiceArrow"
              :interactive="!revealed && !review.busy"
              @move="submitChoice"
            />
          </div>

          <div class="side">
            <template v-if="!revealed">
              <p class="prompt" data-testid="review-prompt">轮到{{ sideLabel(card.side) }}走。在棋盘上走出你的选择，再对照原来的思考与讲解。</p>
              <n-button size="small" data-testid="review-reveal" :disabled="review.busy" @click="submitChoice(null)">不走子，直接看答案</n-button>
              <p v-if="card.attempts.length" class="muted">已经复习 {{ card.attempts.length }} 次。</p>
            </template>

            <template v-else>
              <p class="result" :class="{ ok: review.attempt?.matched }" data-testid="review-result">
                {{ attemptText(review.attempt?.choice ?? null, !!review.attempt?.matched, card.answer.notation) }}
              </p>
              <p class="legend muted">棋盘上实线箭头是棋谱中的着法<template v-if="choiceArrow">，虚线是这次的选择</template>。</p>

              <h4>原来的思考</h4>
              <p data-testid="review-thinking">{{ thinkingText(card.thinking) }}</p>
              <ul v-if="hints" class="hints" data-testid="review-hints">
                <li><span class="hint-level">提示 1 · 局面需求</span>{{ hints.observe }}</li>
                <li v-if="hints.focus"><span class="hint-level">提示 2 · 相关棋子</span>{{ hints.focus.text }}</li>
                <li v-if="hints.moves.length">
                  <span class="hint-level">提示 3 · 可以考虑的着法</span>{{ hints.moves.map((m) => m.notation).join("、") }}（不分先后）
                </li>
              </ul>

              <h4>棋谱中的这一着</h4>
              <div class="answer">
                <strong data-testid="review-answer">{{ card.answer.notation }}</strong>
                <VerdictTag v-if="card.basis?.verdict" :verdict="card.basis.verdict" />
                <n-tag v-if="card.basis" size="small" :bordered="false">{{ LEVEL[card.basis.level] }}</n-tag>
              </div>
              <template v-if="card.basis">
                <p class="summary" data-testid="review-summary">{{ card.basis.summary }}</p>
                <template v-if="card.basis.need.length">
                  <h5>当前需求</h5>
                  <ul class="claims"><li v-for="t in card.basis.need" :key="t">{{ t }}</li></ul>
                </template>
                <template v-if="card.basis.basis.length">
                  <h5>程序依据</h5>
                  <ul class="claims" data-testid="review-basis"><li v-for="t in card.basis.basis" :key="t">{{ t }}</li></ul>
                </template>
                <template v-if="card.basis.replies.length">
                  <h5>对方的主要应对</h5>
                  <ul class="claims"><li v-for="t in card.basis.replies" :key="t">{{ t }}</li></ul>
                </template>
                <p class="lesson"><span class="hint-level">学习点</span>{{ card.basis.lesson }}</p>
                <n-alert v-for="w in card.basis.warnings" :key="w" type="info" :show-icon="false" class="gap">{{ w }}</n-alert>
                <p class="conditions" data-testid="review-version">
                  加入复习时的讲解（讲解规则第 {{ card.basis.generator_version }} 版<template v-if="card.basis.conditions">；分析条件：{{ card.basis.conditions }}</template>）
                </p>
              </template>
              <p v-else class="muted">加入复习时这一着的讲解还没有完成<template v-if="current.source_available">，可以打开原局面查看</template>。</p>

              <h4>你的理解或疑问</h4>
              <n-input
                :value="review.note.text"
                type="textarea"
                :autosize="{ minRows: 3, maxRows: 8 }"
                placeholder="写下这次复习的理解，或还没想通的地方"
                data-testid="review-note"
                @update:value="editNote"
                @blur="saveNote"
              />
              <p class="muted note-state" data-testid="review-note-state">
                <template v-if="review.note.state === 'saving'">保存中…</template>
                <template v-else-if="review.note.state === 'saved'">已保存</template>
                <span v-else-if="review.note.state === 'failed'" class="error">保存失败：{{ review.note.error }}（离开输入框时会再次保存）</span>
                <template v-else-if="review.note.state === 'editing'">有未保存的修改，离开输入框时保存。</template>
                <template v-else>加入复习时从这一步的笔记带入；离开输入框时保存。</template>
              </p>

              <p v-if="recent.length > 1" class="muted" data-testid="review-attempts">
                最近 {{ recent.length }} 次复习：<span v-for="(a, i) in recent" :key="i" :class="a.matched ? 'ok' : 'miss'">{{ a.matched ? "✓" : "✗" }}</span>
              </p>

              <div class="actions">
                <n-button size="small" :type="suggestion === 'again' ? 'primary' : 'default'" data-testid="review-mark-again" @click="mark('again')">需要再练</n-button>
                <n-button size="small" :type="suggestion === 'understood' ? 'primary' : 'default'" data-testid="review-mark-understood" @click="mark('understood')">已理解</n-button>
                <n-button size="small" quaternary @click="retryCard">再试一次</n-button>
                <n-button size="small" quaternary :disabled="!nextCardId" data-testid="review-next" @click="showNext">下一张</n-button>
              </div>
            </template>
            <n-alert v-if="review.error" type="error" :show-icon="false" class="gap">{{ review.error }}</n-alert>
          </div>
        </div>
      </section>
      <section v-else class="practice placeholder">
        <p class="muted">{{ review.items.length ? "从左侧选择一张卡片开始复习。" : study ? "在讲解中点“加入复习”，学过的着法会出现在这里。" : "" }}</p>
      </section>
    </div>
  </n-modal>
</template>

<style scoped>
.review {
  display: grid;
  grid-template-columns: 260px minmax(0, 1fr);
  gap: var(--sp-4);
  min-height: 460px;
}
@media (max-width: 760px) {
  .review {
    grid-template-columns: minmax(0, 1fr);
  }
}
.list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  min-width: 0;
}
.filter {
  display: flex;
}
.filter :deep(.n-radio-button) {
  flex: 1;
  text-align: center;
  padding: 0 6px;
}
.add-starred {
  align-self: flex-start;
}
.cards {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 4px;
  max-height: 60vh;
  overflow: auto;
}
.card-item {
  width: 100%;
  text-align: left;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
  color: inherit;
  font: inherit;
  padding: 8px 10px;
  cursor: pointer;
  display: grid;
  gap: 2px;
}
.card-item:hover {
  border-color: var(--accent-blue);
}
.card-item.active {
  border-color: var(--accent-blue);
  box-shadow: inset 3px 0 0 var(--accent-blue);
}
.item-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 6px;
}
.item-name {
  font-weight: 600;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.item-sub {
  font-size: 12px;
  color: var(--text-muted);
}
.empty {
  margin-top: var(--sp-4);
}
.practice {
  min-width: 0;
}
.placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--sp-2);
  flex-wrap: wrap;
  margin-bottom: var(--sp-2);
}
.title {
  font-weight: 700;
}
.head-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}
.body {
  display: grid;
  grid-template-columns: minmax(240px, 400px) minmax(0, 1fr);
  gap: var(--sp-4);
  align-items: start;
}
@media (max-width: 960px) {
  .body {
    grid-template-columns: minmax(0, 1fr);
  }
  .board {
    max-width: 420px;
  }
}
.side {
  min-width: 0;
  max-height: 64vh;
  overflow: auto;
  padding-right: 4px;
}
.side h4 {
  margin: var(--sp-3) 0 4px;
  font-size: 14px;
}
.side h5 {
  margin: 8px 0 2px;
  font-size: 13px;
  color: var(--text-muted);
}
.side p {
  margin: 0 0 6px;
  line-height: 1.65;
}
.prompt {
  font-size: 15px;
}
.result {
  font-weight: 600;
  color: var(--accent-amber);
}
.result.ok,
.ok {
  color: var(--accent-green);
}
.miss {
  color: var(--accent-amber);
}
.legend {
  font-size: 12px;
}
.hints,
.claims {
  margin: 0;
  padding-left: 18px;
  line-height: 1.65;
}
.hint-level {
  font-size: 12px;
  color: var(--text-muted);
  margin-right: 6px;
}
.answer {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 16px;
}
.summary {
  margin-top: 6px;
}
.lesson {
  margin-top: 8px;
}
.conditions {
  font-size: 12px;
  color: var(--text-muted);
}
.muted {
  color: var(--text-muted);
}
.note-state {
  font-size: 12px;
  margin-top: 4px;
}
.error {
  color: var(--accent-red);
}
.gap {
  margin-top: 8px;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: var(--sp-3);
}
</style>
