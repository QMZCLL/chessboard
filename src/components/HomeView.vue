<script setup lang="ts">
// 今日学习：打开软件时知道练什么。三块内容——继续上次的练习、今天到期的卡片、继续复盘最近的棋谱，
// 下面是按自己确认的问题给出的专题推荐（写明依据）。主要操作随状态决定：有没有完成的练习时继续练习；
// 否则有到期的卡片时开始今天的练习；还没有复习卡片时建立学习局面。
import { computed, onMounted, ref, watch } from "vue";
import { NButton, NSelect, useMessage } from "naive-ui";
import type { StudySummary } from "../api/types";
import { api, errorMessage } from "../api/backend";
import { openPractice, openReviews, review, reviewCounts, startPractice } from "../state/review";
import type { TopicKey } from "../state/reviewList";
import { dueAt, dueText, entryState, GOAL_LABEL, ISSUE_LABEL, recommendation } from "../state/reviewList";
import { lastPositions, openStudy, study, wb } from "../state/workbench";
import Icon from "./Icon.vue";

const message = useMessage();
const SIZES = [
  { label: "5 张", value: 5 },
  { label: "10 张", value: 10 },
  { label: "全部", value: 0 },
];
const size = ref(5);
const recent = ref<StudySummary[]>([]);
const recentError = ref<string | null>(null);
const positions = ref(lastPositions());

async function loadRecent() {
  try {
    recent.value = (await api.listStudies()).slice(0, 3);
    positions.value = lastPositions();
    recentError.value = null;
  } catch (e) {
    recentError.value = errorMessage(e);
  }
}
onMounted(loadRecent);
watch(
  () => wb.main,
  (m) => m === "home" && void loadRecent(),
);

const session = computed(() => review.session);
const progress = computed(() => {
  const s = session.value;
  if (!s) return null;
  const states = s.cards.map((id) => entryState(s, id, review.items.find((i) => i.card.id === id)?.card));
  const answered = states.filter((x) => x === "answered").length;
  return { answered, left: states.filter((x) => x === "todo").length, total: s.cards.length };
});
/** 今天没有到期的卡片时，最近的一次安排 */
const nextDue = computed(() => {
  const cards = review.items.map((i) => i.card).sort((a, b) => dueAt(a) - dueAt(b));
  return cards[0] ? dueText(cards[0], review.clock) : null;
});
const recommended = computed(() => recommendation(review.items));
/** 这一页的主要操作 */
const primary = computed(() => (session.value ? "resume" : reviewCounts.value.today ? "today" : review.items.length ? "recent" : "build"));

function positionText(s: StudySummary): string {
  const p = positions.value[s.id];
  if (!p) return `${s.moves} 着`;
  return p.ply ? `${s.moves} 着 · 上次看到第 ${Math.ceil(p.ply / 2)} 回合` : `${s.moves} 着 · 上次在起始局面`;
}

async function start(kind: "today" | TopicKey, n: number | null) {
  if (!(await startPractice(kind, n))) message.error(review.error ?? "没有可以练习的卡片");
}

async function resumeStudy(s: StudySummary) {
  try {
    await openStudy(s.id, positions.value[s.id]?.node);
  } catch (e) {
    recentError.value = errorMessage(e);
  }
}
</script>

<template>
  <main class="home" data-testid="home">
    <header class="home-head">
      <div>
        <h1>今日学习</h1>
        <p class="muted">继续上次的练习、复习今天到期的卡片，或回到最近的棋谱。</p>
      </div>
      <n-button v-if="study" size="small" secondary data-testid="home-board" @click="wb.main = 'board'">
        <template #icon><Icon name="back" /></template>返回棋盘
      </n-button>
    </header>

    <div class="blocks">
      <!-- 继续上次练习 -->
      <section class="block surface" data-testid="home-session">
        <h2>继续上次练习</h2>
        <template v-if="session && progress">
          <p class="lead">{{ session.label }}</p>
          <p class="muted" data-testid="home-session-progress">
            {{ GOAL_LABEL[session.goal] }} · 已作答 {{ progress.answered }} / {{ progress.total }} 题，还有 {{ progress.left }} 题
          </p>
          <n-button :type="primary === 'resume' ? 'primary' : 'default'" data-testid="home-resume" @click="openPractice">继续练习</n-button>
        </template>
        <p v-else class="muted">没有进行中的练习。开始一组练习后可以随时保存退出，下次从原处继续。</p>
      </section>

      <!-- 今天到期 -->
      <section class="block surface" data-testid="home-today">
        <h2>今天到期</h2>
        <template v-if="review.items.length">
          <p class="lead" data-testid="home-today-count">{{ reviewCounts.today ? `${reviewCounts.today} 张卡片今天到期` : "今天没有到期的卡片" }}</p>
          <p v-if="!reviewCounts.today && nextDue" class="muted">最近的一张：{{ nextDue }}</p>
          <div class="row">
            <n-button
              :type="primary === 'today' ? 'primary' : 'default'"
              :disabled="!reviewCounts.today"
              data-testid="home-start-today"
              @click="start('today', size || null)"
            >
              开始今天的练习
            </n-button>
            <n-select v-model:value="size" size="small" :options="SIZES" class="size" aria-label="这组练习的张数" data-testid="home-session-size" />
          </div>
          <n-button text size="small" class="library" data-testid="home-library" @click="openReviews()">打开复习库</n-button>
        </template>
        <template v-else>
          <p class="muted">还没有复习卡片。看讲解时点“加入复习”，学过的着法会出现在这里。</p>
          <n-button :type="primary === 'build' ? 'primary' : 'default'" data-testid="home-build" @click="wb.dialogs.newStudy = true">建立学习局面</n-button>
        </template>
      </section>

      <!-- 继续复盘 -->
      <section class="block surface" data-testid="home-recent">
        <h2>继续复盘</h2>
        <ul v-if="recent.length" class="recent">
          <li v-for="(s, i) in recent" :key="s.id">
            <button type="button" class="recent-item focusable" :data-home-study="s.id" @click="resumeStudy(s)">
              <span class="recent-name">{{ s.name }}</span>
              <span class="muted recent-sub">{{ positionText(s) }}</span>
              <span v-if="i === 0 && primary === 'recent'" class="recent-go">继续复盘</span>
            </button>
          </li>
        </ul>
        <p v-else class="muted">还没有学习局面。</p>
        <p v-if="recentError" class="error">{{ recentError }}</p>
        <n-button text size="small" class="library" @click="wb.dialogs.newStudy = true">新建或打开其他棋谱</n-button>
      </section>
    </div>

    <!-- 专题推荐：只按用户自己确认的问题，写明依据 -->
    <section v-if="recommended" class="recommend surface" data-testid="home-recommend">
      <p>
        你有 {{ recommended.count }} 张卡片标记了“{{ ISSUE_LABEL[recommended.issue] }}”，可以集中练习这一组。
      </p>
      <n-button size="small" secondary data-testid="home-recommend-start" @click="start(recommended.key, null)">练这一组</n-button>
    </section>
  </main>
</template>

<style scoped>
.home {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--sp-5, 24px) var(--gutter);
}
.home-head,
.blocks,
.recommend {
  max-width: 960px;
  margin-left: auto;
  margin-right: auto;
}
.home-head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--sp-3);
  margin-bottom: var(--sp-4);
}
.home-head h1 {
  margin: 0 0 4px;
  font-family: var(--font-serif);
  font-size: 22px;
  letter-spacing: 0.06em;
}
.home-head p {
  margin: 0;
}
.blocks {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--sp-3);
}
.block {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--sp-2);
  padding: var(--sp-4);
  min-width: 0;
}
.block h2 {
  margin: 0;
  font-family: var(--font-serif);
  font-size: 16px;
}
.block p {
  margin: 0;
  line-height: 1.6;
}
.lead {
  font-weight: 600;
}
.row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.size {
  width: 88px;
}
.library {
  margin-top: auto;
}
.recent {
  list-style: none;
  margin: 0;
  padding: 0;
  width: 100%;
  display: grid;
  gap: 4px;
}
.recent-item {
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
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 0 6px;
}
.recent-item:hover {
  border-color: var(--accent-blue);
}
.recent-name {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.recent-sub {
  grid-column: 1;
  font-size: 12px;
}
.recent-go {
  grid-column: 2;
  grid-row: 1 / span 2;
  align-self: center;
  font-size: 12px;
  color: var(--accent-primary);
  font-weight: 600;
}
.recommend {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-top: var(--sp-3);
  padding: var(--sp-3) var(--sp-4);
}
.recommend p {
  margin: 0;
}
.muted {
  color: var(--text-muted);
}
.error {
  color: var(--accent-red);
}
</style>
