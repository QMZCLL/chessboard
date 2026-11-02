<script setup lang="ts">
// 专注练习工作区：一组练习按开始时固定的顺序进行，以棋盘为中心。顶部始终显示练习目标与进度，
// 可以保存退出（位置已经写回后台，之后从原处继续）或结束并查看小结；本组卡片列表可以展开，
// 写明每一题的进展（未作答、已跳过、卡片已删除，或第一次作答的结果）。
import { computed, ref } from "vue";
import { NAlert, NButton, NEmpty } from "naive-ui";
import ReviewPractice from "./ReviewPractice.vue";
import Icon from "./Icon.vue";
import { exitPractice, finishSession, goTo, openReviews, review, sessionCardId, sessionResult } from "../state/review";
import { GOAL_LABEL, entryBadge, entryState, positionLabel, summaryText } from "../state/reviewList";
import { wb } from "../state/workbench";

const session = computed(() => review.session);
const listOpen = ref(false);
const cardOf = (id: string) => review.items.find((i) => i.card.id === id)?.card;
const entries = computed(() => {
  const s = session.value;
  if (!s) return [];
  return s.cards.map((id, index) => {
    const card = cardOf(id);
    return { id, index, card, state: entryState(s, id, card), badge: entryBadge(s, id, card) };
  });
});
const answered = computed(() => entries.value.filter((e) => e.state === "answered").length);
const GOAL_HINT = { recall: "看是否记得棋谱中的着法", decision: "看这次的选择是否得到同一次搜索比较的支持" } as const;

async function backToLibrary() {
  await exitPractice();
  openReviews();
}
</script>

<template>
  <main class="ws" data-testid="practice-workspace">
    <!-- 一组练习的小结：数字都来自这组练习的作答与卡片的安排 -->
    <section v-if="sessionResult" class="summary surface" data-testid="practice-summary">
      <h2>这组练习的小结</h2>
      <p data-testid="review-session-summary">{{ summaryText(sessionResult) }}</p>
      <div class="summary-actions">
        <n-button type="primary" data-testid="review-close-summary" @click="exitPractice">完成</n-button>
        <n-button secondary data-testid="practice-back-library" @click="backToLibrary">返回复习库</n-button>
      </div>
    </section>

    <template v-else-if="session">
      <header class="ws-head">
        <div class="ws-title">
          <strong data-testid="practice-label">{{ session.label }}</strong>
          <span class="goal" data-testid="practice-goal">{{ GOAL_LABEL[review.goal] }}：{{ GOAL_HINT[review.goal] }}</span>
        </div>
        <span class="progress" data-testid="practice-progress">第 {{ session.index + 1 }} / {{ session.cards.length }} 题 · 已作答 {{ answered }} 题</span>
        <div class="ws-actions">
          <n-button size="small" quaternary :aria-expanded="listOpen" data-testid="practice-toggle-list" @click="listOpen = !listOpen">
            <template #icon><Icon name="list" /></template>本组卡片
          </n-button>
          <n-button size="small" quaternary data-testid="review-finish" @click="finishSession">结束并查看小结</n-button>
          <n-button size="small" secondary data-testid="practice-exit" @click="exitPractice">
            <template #icon><Icon name="save" /></template>保存退出
          </n-button>
        </div>
      </header>
      <n-alert v-if="review.notice" type="info" :show-icon="false" class="notice" data-testid="practice-notice">{{ review.notice }}</n-alert>

      <div class="ws-body" :class="{ 'with-list': listOpen }">
        <aside v-if="listOpen" class="ws-list surface" aria-label="本组卡片" data-testid="practice-list">
          <ol>
            <li v-for="e in entries" :key="e.id">
              <button
                type="button"
                class="entry focusable"
                :class="{ active: e.id === sessionCardId }"
                :disabled="e.state === 'missing' || review.busy"
                :aria-current="e.id === sessionCardId ? 'step' : undefined"
                :data-practice-entry="e.index"
                @click="goTo(e.index)"
              >
                <span class="entry-top">
                  <span>第 {{ e.index + 1 }} 题</span>
                  <span class="badge" :class="e.badge.tone">{{ e.badge.text }}</span>
                </span>
                <span v-if="e.card" class="entry-sub">{{ e.card.study_name }} · {{ positionLabel(e.card) }}</span>
              </button>
            </li>
          </ol>
        </aside>
        <div class="ws-main surface">
          <!-- 复习库打开时由它显示卡片；关闭后回到这组练习的当前题目 -->
          <ReviewPractice v-if="review.current && !wb.dialogs.review" session />
          <n-empty v-else-if="!review.busy && !wb.dialogs.review" description="这一题的卡片还没有读取" size="small" class="empty" />
          <n-alert v-if="!review.current && review.error" type="error" :show-icon="false">{{ review.error }}</n-alert>
        </div>
      </div>
    </template>

    <n-empty v-else description="没有进行中的练习" class="empty">
      <template #extra>
        <n-button size="small" @click="exitPractice">返回</n-button>
      </template>
    </n-empty>
  </main>
</template>

<style scoped>
.ws {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  padding: var(--gutter);
}
.ws-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2) var(--sp-4);
}
.ws-title {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: var(--sp-2);
  min-width: 0;
}
.ws-title strong {
  font-family: var(--font-serif);
  font-size: 17px;
}
.goal,
.progress {
  color: var(--text-muted);
  font-size: 13px;
}
.ws-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}
.notice {
  font-size: 13px;
}
.ws-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: var(--sp-3);
}
.ws-body.with-list {
  grid-template-columns: 240px minmax(0, 1fr);
}
/* 窄窗口：列表放在上方，限制高度，留出练习的空间 */
@media (max-width: 760px) {
  .ws-body.with-list {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
  }
  .ws-list {
    max-height: 28vh;
  }
}
.ws-main {
  min-height: 0;
  min-width: 0;
  overflow: auto;
  padding: var(--sp-3) var(--sp-4);
}
.ws-list {
  min-height: 0;
  overflow: auto;
  padding: var(--sp-2);
}
.ws-list ol {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 4px;
}
.entry {
  width: 100%;
  text-align: left;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
  color: inherit;
  font: inherit;
  padding: 6px 10px;
  cursor: pointer;
  display: grid;
  gap: 2px;
}
.entry:disabled {
  cursor: default;
  opacity: 0.65;
}
.entry.active {
  border-color: var(--accent-blue);
  box-shadow: inset 3px 0 0 var(--accent-blue);
}
.entry-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
}
.entry-sub {
  font-size: 12px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.badge {
  font-size: 12px;
  font-weight: 600;
  padding: 0 8px;
  border-radius: 999px;
  background: var(--tag-gray-bg);
  color: var(--text-secondary);
  white-space: nowrap;
}
.badge.ok {
  color: var(--accent-green);
}
.badge.miss {
  color: var(--accent-amber);
}
.summary {
  max-width: 640px;
  margin: var(--sp-6, 32px) auto 0;
  padding: var(--sp-4) var(--sp-5, 24px);
}
.summary h2 {
  margin: 0 0 var(--sp-2);
  font-family: var(--font-serif);
  font-size: 18px;
}
.summary p {
  line-height: 1.7;
}
.summary-actions {
  display: flex;
  gap: var(--sp-2);
  margin-top: var(--sp-3);
}
.empty {
  margin: auto;
}
</style>
