<script setup lang="ts">
// 复习库：左侧是卡片列表与分组，右侧单独复习一张卡片（见 ReviewPractice）。
// 开始一组练习（今天到期、需要再练或某个专题）后进入专注练习工作区；进行中的练习可以从这里继续。
// 列表中不显示棋谱中的着法和笔记，避免提前透露答案。
import { computed, onMounted, ref } from "vue";
import { NButton, NEmpty, NModal, NRadioButton, NRadioGroup, NSelect, NTag, useMessage } from "naive-ui";
import ReviewPractice from "./ReviewPractice.vue";
import type { ReviewStatus } from "../api/types";
import { errorMessage } from "../api/backend";
import {
  addStarred,
  loadReviews,
  openPractice,
  resumeSession,
  review,
  reviewCounts,
  saveNote,
  sessionCardId,
  selectCard,
  showCard,
  shownItems,
  startPractice,
  starredNotInReview,
} from "../state/review";
import { dueText, ISSUE_LABEL, positionLabel, recommendation, STATUS_LABEL, topics, whenText, type TopicKey } from "../state/reviewList";
import { KIND_LABEL } from "../state/cardKind";
import { study, wb } from "../state/workbench";

const message = useMessage();
const card = computed(() => review.current?.card ?? null);
const STATUS_TYPE: Record<ReviewStatus, "warning" | "error" | "success"> = { due: "warning", again: "error", understood: "success" };
/** 一组练习的张数；0 表示今天到期的全部 */
const sessionSize = ref(5);
const SIZES = [
  { label: "5 张", value: 5 },
  { label: "10 张", value: 10 },
  { label: "全部", value: 0 },
];
const session = computed(() => review.session);
const topic = ref<TopicKey | null>(null);
const topicOptions = computed(() => topics(review.items).map((t) => ({ label: `${t.label}（${t.count} 张）`, value: t.key })));
const recommended = computed(() => recommendation(review.items));

async function addFromStarred() {
  try {
    const n = await addStarred();
    message.success(`已把 ${n} 着收藏的着法加入复习`);
    if (!review.current && shownItems.value[0]) await showCard(shownItems.value[0].card.id);
  } catch (e) {
    message.error(errorMessage(e));
  }
}

/** 关闭复习库：在练习工作区中打开时，工作区回到这组练习的当前题目 */
function afterLeave() {
  void saveNote();
  if (wb.main === "practice" && review.session && review.current?.card.id !== sessionCardId.value) void resumeSession();
}

onMounted(loadReviews);
</script>

<template>
  <n-modal v-model:show="wb.dialogs.review" preset="card" title="复习" style="width: min(1120px, 96vw)" data-testid="review-dialog" @after-leave="afterLeave">
    <div class="review">
      <!-- 卡片列表 -->
      <aside class="list">
        <n-radio-group v-model:value="review.filter" size="small" class="filter">
          <n-radio-button value="today" data-testid="review-filter-today">今天 {{ reviewCounts.today }}</n-radio-button>
          <n-radio-button value="again" data-testid="review-filter-again">再练 {{ reviewCounts.again }}</n-radio-button>
          <n-radio-button value="understood" data-testid="review-filter-understood">已理解 {{ reviewCounts.understood }}</n-radio-button>
          <n-radio-button value="all" data-testid="review-filter-all">全部 {{ reviewCounts.all }}</n-radio-button>
        </n-radio-group>
        <!-- 进行中的一组练习：在专注练习工作区中从原处继续 -->
        <div v-if="session" class="session" data-testid="review-session">
          <span data-testid="review-session-progress"><strong>{{ session.label }}</strong> · 第 {{ session.index + 1 }} / {{ session.cards.length }} 题</span>
          <n-button size="small" type="primary" data-testid="review-resume" @click="openPractice">继续练习</n-button>
        </div>
        <!-- 推荐：只按用户自己确认的问题，写明依据 -->
        <div v-if="recommended" class="recommend" data-testid="review-recommend">
          你有 {{ recommended.count }} 张卡片标记了“{{ ISSUE_LABEL[recommended.issue] }}”。
          <n-button text type="primary" size="small" data-testid="review-recommend-start" @click="startPractice(recommended.key, null)">练这一组</n-button>
        </div>
        <div class="session-start">
          <n-button
            size="small"
            :type="session ? 'default' : 'primary'"
            :disabled="!reviewCounts.today"
            data-testid="review-start-today"
            @click="startPractice('today', sessionSize || null)"
          >
            开始今天的练习
          </n-button>
          <n-select v-model:value="sessionSize" size="small" :options="SIZES" class="size" data-testid="review-session-size" />
          <n-button size="small" quaternary :disabled="!reviewCounts.again" data-testid="review-start-again" @click="startPractice('again', null)">
            只练需要再练的 {{ reviewCounts.again }} 张
          </n-button>
        </div>
        <div v-if="topicOptions.length" class="session-start">
          <n-select v-model:value="topic" size="small" :options="topicOptions" placeholder="按专题练习" clearable class="topic" data-testid="review-topic" />
          <n-button size="small" :disabled="!topic" data-testid="review-start-topic" @click="topic && startPractice(topic, null)">练这个专题</n-button>
        </div>
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
              @click="selectCard(item.card.id)"
            >
              <span class="item-top">
                <span class="item-name">{{ item.study_name }}</span>
                <n-tag v-if="item.card.kind !== 'move'" size="tiny" :bordered="false" data-testid="card-kind">{{ KIND_LABEL[item.card.kind] }}</n-tag>
                <n-tag size="tiny" :type="STATUS_TYPE[item.card.status]" :bordered="false">{{ STATUS_LABEL[item.card.status] }}</n-tag>
              </span>
              <span class="item-sub" :title="whenText(item.card.reviewed_at)">{{ positionLabel(item.card) }} · {{ dueText(item.card, review.clock) }}</span>
            </button>
          </li>
        </ol>
        <n-empty v-else size="small" class="empty" :description="review.items.length ? '这一组没有卡片' : '还没有复习卡片'">
          <template v-if="!review.items.length" #extra>
            <p class="muted">在讲解中点“加入复习”，或把收藏的着法加入复习。</p>
          </template>
        </n-empty>
      </aside>

      <!-- 单独复习一张卡片 -->
      <ReviewPractice v-if="card && review.current" />
      <section v-else class="placeholder">
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
  padding: 0 4px;
}
.session,
.session-start {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  font-size: 13px;
}
.session {
  padding: 6px var(--sp-2);
  border-radius: var(--radius-sm);
  background: var(--tag-gray-bg);
}
.session-start .size {
  width: 84px;
}
.session-start .topic {
  flex: 1;
  min-width: 0;
}
.recommend {
  font-size: 13px;
  padding: 6px var(--sp-2);
  border-radius: var(--radius-sm);
  background: var(--tag-gray-bg);
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
.placeholder {
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
.muted {
  color: var(--text-muted);
}
</style>
