<script setup lang="ts">
// 复习一张卡片：复习库（复习窗口）与专注练习工作区共用。先只显示局面，在棋盘上重新选择（或直接看答案）后，
// 再显示这次的结果、原来的思考与看过的提示、棋谱中的这一着与程序讲解，并记下自己的理解或疑问。
// 复习开始前不显示棋谱中的着法和笔记，避免提前透露答案。
// 练习目标：复现棋谱看是否与棋谱一致；局面决策把这次的选择与参考放在同一次搜索中比较。
// 落子前可以选择意图，揭示后与这一步实际的规则事实对照；可以看分级提示（与先想后讲相同），
// 可以选择这一步的把握程度。用了提示之后选对不算独立选对；作答用时只记录，不评分。
// 每个阶段只突出一个主要操作：选择前是在棋盘上走子；揭示后先标记状态（有建议时），标记之后是下一题。
// 揭示后的内容分成“你的选择”“比较结论”“具体原因”“下一次检查什么”；当前阶段的操作在底部，
// 固定在可见区域内（讲解较长、窗口较窄时也不用在讲解中寻找）。
import { computed } from "vue";
import { NAlert, NButton, NInput, NPopconfirm, NRadioButton, NRadioGroup, NSpin, NTag, NTooltip, useMessage } from "naive-ui";
import XiangqiBoard from "./XiangqiBoard.vue";
import VerdictTag from "./VerdictTag.vue";
import type { ReviewIssue, ReviewStatus } from "../api/types";
import { errorMessage } from "../api/backend";
import {
  currentAttempt,
  currentNote,
  deleteCard,
  editNote,
  nextCardId,
  openSource,
  retryCard,
  review,
  saveNote,
  setGoal,
  setIssues,
  setStatus,
  showNext,
  showReviewHint,
  skipCard,
  submitChoice,
  toggleMark,
} from "../state/review";
import {
  attemptMark,
  attemptNotesText,
  attemptText,
  awaitingResult,
  CONFIDENCE_LABEL,
  CONFIDENCES,
  kindResultText,
  decisionTitle,
  dueText,
  GOAL_LABEL,
  historyText,
  INTENT_LABEL,
  INTENTS,
  ISSUE_LABEL,
  ISSUES,
  outcomeBadge,
  positionLabel,
  STATUS_LABEL,
  suggestionFor,
  THEME_LABEL,
  thinkingText,
} from "../state/reviewList";
import { sideLabel } from "../utils/board";

/** session：在一组练习中（专注练习工作区）；否则是复习库中单独复习一张卡片 */
const props = defineProps<{ session?: boolean }>();

const message = useMessage();
const current = computed(() => review.current);
const card = computed(() => current.value?.card ?? null);
const revealed = computed(() => review.phase === "revealed");
const STATUS_TYPE: Record<ReviewStatus, "warning" | "error" | "success"> = { due: "warning", again: "error", understood: "success" };
const LEVEL = { basic: "规则讲解", partial: "分析不完整", detailed: "引擎分析完整" } as const;

// 揭示后：实线是棋谱中的这一着，虚线是这次的选择（与棋谱不同时）
const answerArrows = computed(() => (revealed.value && card.value && card.value.kind !== "threat" ? [card.value.answer.mv] : []));
const attempt = computed(() => currentAttempt.value);
const choiceArrow = computed(() => {
  const a = attempt.value;
  return revealed.value && a?.choice && !a.matched ? a.choice.mv : null;
});
const lastMove = computed(() => card.value?.history[card.value.history.length - 1]?.mv ?? null);
const suggestion = computed(() => (attempt.value ? suggestionFor(attempt.value) : null));
const decision = computed(() => attempt.value?.decision ?? null);
/** 这次作答的结果标签（与练习列表、小结相同的文字与颜色） */
const badge = computed(() => (attempt.value ? outcomeBadge(attempt.value, card.value?.kind) : null));
const GOAL_HINT = { recall: "回忆这盘棋当时怎样走，看是否与棋谱一致", decision: "作出自己的选择，与棋谱着法、引擎参考在同一次搜索中比较" } as const;
const toggleIntent = (i: (typeof INTENTS)[number]) => (review.intent = review.intent === i ? null : i);
const hints = computed(() => card.value?.thinking?.hints ?? null);
/** 选择前看过的提示：二级提示在棋盘上标出相关的棋子与关系 */
const hintMarks = computed(() => (review.hintData && review.hints >= 2 ? review.hintData.focus.squares : []));
const hintArrows = computed(() => (review.hintData && review.hints >= 2 ? review.hintData.focus.arrows : []));
/** 这次作答看了几级提示、选择的把握与用时 */
const attemptNotes = computed(() => (attempt.value ? attemptNotesText(attempt.value) : ""));
/** 在棋盘上走子作答的题型（找威胁点选格子，二选一用按钮） */
const moves = computed(() => card.value?.kind === "move" || card.value?.kind === "better");
const prompt = computed(() => {
  const c = card.value;
  if (!c) return "";
  const side = sideLabel(c.side);
  switch (c.kind) {
    case "threat":
      return `轮到${side}走。先不走子：在棋盘上点出受到威胁的己方棋子（被将军时还要点出将军的棋子），再提交。`;
    case "choice":
      return `轮到${side}走。下面两步中哪一步更好？选好之后对照同一次搜索的结论。`;
    case "better":
      return `轮到${side}走。当时走了 ${c.played?.notation ?? ""}，程序评价为明显不如引擎首选。在棋盘上走出你认为更好的一步。`;
    default:
      return `轮到${side}走。在棋盘上走出你的选择，再对照原来的思考与讲解。`;
  }
});
/** 选择前棋盘上的标记：找威胁时是点出的格子，其他题型是二级提示标出的棋子 */
const choose = computed(() =>
  card.value?.kind === "threat" ? { marks: review.marks, arrows: [] as string[] } : { marks: hintMarks.value, arrows: hintArrows.value },
);
/** 揭示后：找威胁标出答案的格子，自己点出的格子另外高亮 */
const revealedMarks = computed(() => (card.value?.kind === "threat" ? card.value.targets : []));
const revealedPicks = computed(() => (card.value?.kind === "threat" ? (attempt.value?.marks ?? []) : []));
const answerTitle = computed(() => {
  const c = card.value;
  if (!c || c.kind === "move") return "棋谱中的这一着";
  if (c.kind === "threat") return `受到威胁的位置：${c.targets.join("、")}；当时走的着法`;
  return "答案";
});
const recent = computed(() => (card.value?.attempts ?? []).slice(-8));
/** 已经按这次作答标记了状态（安排依据的就是这次作答） */
const marked = computed(() => {
  const c = card.value;
  return !!c && !!attempt.value && c.status !== "due" && c.schedule_base?.attempt_at === attempt.value.at;
});
/** 标记前突出建议的状态；练习中标记之后突出下一题 */
const primaryMark = computed(() => (marked.value ? null : suggestion.value));
const nextPrimary = computed(() => !!props.session && (marked.value || !suggestion.value));
const nextLabel = computed(() => {
  const s = review.session;
  if (!props.session || !s) return "下一张";
  return nextCardId.value ? `下一题（${s.cards.indexOf(nextCardId.value) + 1} / ${s.cards.length}）` : "完成这组练习";
});

async function toggleIssue(issue: ReviewIssue) {
  const c = card.value;
  if (!c) return;
  const issues = c.issues.includes(issue) ? c.issues.filter((i) => i !== issue) : [...c.issues, issue];
  try {
    await setIssues(c.id, issues);
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
  // 点击时就确定是哪张卡片：保存笔记期间换了卡片，笔记与状态仍记在原来的这张上
  const id = card.value?.id;
  if (!id) return;
  await saveNote(id);
  try {
    await setStatus(id, status);
    const label = STATUS_LABEL[status];
    const markedCard = review.items.find((i) => i.card.id === id)?.card;
    const when = !markedCard ? "" : awaitingResult(markedCard) ? "，比较完成后确定下一次复习的时间" : `，${dueText(markedCard, Date.now())}`;
    message.success(review.current?.card.id === id ? `已标为“${label}”${when}` : `已把之前复习的卡片标为“${label}”${when}`);
  } catch (e) {
    message.error(errorMessage(e));
  }
}
</script>

<template>
  <section v-if="card && current" class="practice" :class="{ session }" data-testid="review-card">
    <header class="head">
      <div>
        <div class="title">{{ current.study_name }} · {{ positionLabel(card) }}</div>
        <div class="muted" data-testid="review-history">{{ historyText(card) }}</div>
      </div>
      <div class="head-actions">
        <n-tag size="small" :type="STATUS_TYPE[card.status]" :bordered="false" data-testid="review-status">{{ STATUS_LABEL[card.status] }}</n-tag>
        <span class="muted due" data-testid="review-due">{{ awaitingResult(card) ? "比较完成后确定下一次复习的时间" : dueText(card, review.clock) }}</span>
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
          :arrows="revealed ? answerArrows : choose.arrows"
          :marks="revealed ? revealedMarks : choose.marks"
          :highlights="revealed ? revealedPicks : []"
          :next-arrow="choiceArrow"
          :interactive="!revealed && !review.busy && moves"
          :pick="!revealed && !review.busy && card.kind === 'threat'"
          @move="submitChoice"
          @pick="toggleMark"
        />
      </div>

      <div class="side">
        <template v-if="!revealed">
          <p class="prompt" data-testid="review-prompt">{{ prompt }}</p>
          <!-- 二选一：两个选项按坐标排列，不按优劣 -->
          <div v-if="card.kind === 'choice'" class="options" data-testid="review-options">
            <n-button
              v-for="o in card.options"
              :key="o.mv"
              :disabled="review.busy"
              :data-testid="`review-option-${o.mv}`"
              @click="submitChoice(o.mv)"
            >
              {{ o.notation }}
            </n-button>
          </div>
          <div v-if="card.kind === 'move'" class="goal">
            <n-radio-group :value="review.goal" size="small" data-testid="review-goal" @update:value="setGoal">
              <n-radio-button value="recall" data-testid="review-goal-recall">{{ GOAL_LABEL.recall }}</n-radio-button>
              <n-radio-button value="decision" data-testid="review-goal-decision">{{ GOAL_LABEL.decision }}</n-radio-button>
            </n-radio-group>
            <span class="muted">{{ GOAL_HINT[review.goal] }}</span>
          </div>
          <div v-if="moves" class="intent" data-testid="review-intent">
            <span class="muted">我这一步主要想（可以不选）：</span>
            <n-button
              v-for="i in INTENTS"
              :key="i"
              size="tiny"
              :type="review.intent === i ? 'primary' : 'default'"
              :secondary="review.intent !== i"
              :aria-pressed="review.intent === i"
              :data-testid="`review-intent-${i}`"
              @click="toggleIntent(i)"
            >
              {{ INTENT_LABEL[i] }}
            </n-button>
          </div>
          <div class="confidence" data-testid="review-confidence">
            <span class="muted">这一步的把握（可以不选）：</span>
            <n-radio-group v-model:value="review.confidence" size="small">
              <n-radio-button v-for="c in CONFIDENCES" :key="c" :value="c" :data-testid="`review-confidence-${c}`">{{ CONFIDENCE_LABEL[c] }}</n-radio-button>
            </n-radio-group>
          </div>
          <!-- 分级提示：与先想后讲相同，只依据当前局面，不透露棋谱中的着法 -->
          <section v-if="review.hints" class="review-hints" data-testid="review-hint-list">
            <p v-if="review.hintData"><span class="hint-level">提示 1 · 局面需求</span>{{ review.hintData.observe }}</p>
            <p v-if="review.hintData && review.hints >= 2"><span class="hint-level">提示 2 · 相关棋子</span>{{ review.hintData.focus.text }}</p>
            <p v-if="review.hintData && review.hints >= 3 && review.hintData.moves.length">
              <span class="hint-level">提示 3 · 可以考虑的着法</span>{{ review.hintData.moves.map((m) => m.notation).join("、") }}（不分先后）
            </p>
            <p class="muted">用了提示之后选对，记为“看了提示后选对”，不算独立选对。</p>
          </section>
          <p v-if="card.attempts.length" class="muted">已经复习 {{ card.attempts.length }} 次。</p>
        </template>

        <template v-else>
          <!-- 你的选择：这一行只陈述事实；局面决策中与棋谱不同不等于走错，结论看下面的比较 -->
          <section class="group" data-testid="feedback-choice">
            <h3>你的选择 <span v-if="badge" class="badge" :class="badge.tone" data-testid="review-badge">{{ badge.text }}</span></h3>
            <p class="result" :class="{ ok: attempt?.matched, neutral: attempt?.goal === 'decision' && !attempt?.matched }" data-testid="review-result">
              {{ card.kind === "move" ? attemptText(attempt?.choice ?? null, !!attempt?.matched, card.answer.notation) : kindResultText(card, attempt ?? { choice: null, marks: [], matched: false }) }}
            </p>
            <p v-if="attemptNotes" class="muted" data-testid="review-attempt-notes">{{ attemptNotes }}</p>
            <p v-if="card.kind === 'threat'" class="legend muted">棋盘上圈出的是受到威胁的位置，高亮的是你点出的格子。</p>
            <p v-else class="legend muted">棋盘上实线箭头是{{ card.kind === "move" ? "棋谱中的着法" : "引擎首选" }}<template v-if="choiceArrow">，虚线是这次的选择</template>。</p>
            <!-- 原来的意图与这一步实际的变化（规则事实） -->
            <template v-if="attempt?.reflection">
              <h4 data-testid="review-intent-title">
                {{ attempt.reflection.intent ? `你原来想：${INTENT_LABEL[attempt.reflection.intent]}` : "你的选择实际改变了什么" }}
              </h4>
              <ul v-if="attempt.reflection.checks.length" class="claims" data-testid="review-intent-checks">
                <li v-for="t in attempt.reflection.checks" :key="t">{{ t }}</li>
              </ul>
              <h5 v-if="attempt.reflection.intent && attempt.reflection.changes.length">这一步实际改变了</h5>
              <ul v-if="attempt.reflection.changes.length" class="claims" data-testid="review-changes">
                <li v-for="t in attempt.reflection.changes" :key="t">{{ t }}</li>
              </ul>
              <p v-else class="muted">这一步没有可以单独说明的直接作用。</p>
            </template>
          </section>

          <!-- 比较结论：同一次搜索中的比较，评分与结论都由程序给出 -->
          <section v-if="decision" class="group" data-testid="feedback-verdict">
            <h3>比较结论</h3>
            <p class="decision" :class="badge?.tone" data-testid="review-decision">
              <n-spin v-if="decision.state === 'pending'" :size="12" /> {{ decisionTitle(decision) }}
            </p>
            <ul v-if="decision.points.length" class="claims" data-testid="review-decision-points">
              <li v-for="t in decision.points" :key="t">{{ t }}</li>
            </ul>
            <p v-if="decision.state === 'ready' && decision.reference" class="muted">对照着法：{{ decision.reference.notation }}</p>
            <p v-if="decision.conditions" class="conditions">分析条件：{{ decision.conditions }}</p>
          </section>

          <!-- 具体原因：棋谱中的这一着、程序讲解与原来的思考 -->
          <section class="group">
            <h3>具体原因</h3>
            <p v-if="card.origin" class="muted" data-testid="review-origin">出处：{{ card.origin }}</p>
            <h4>{{ answerTitle }}</h4>
            <!-- 生成的题目：讲解与评价属于当时走的那一着，标明是哪一着，不放在引擎首选旁边 -->
            <p v-if="card.played" class="answer" data-testid="review-answer">
              引擎首选 <strong>{{ card.answer.notation }}</strong>
            </p>
            <h4 v-if="card.played">当时这一着（{{ card.played.notation }}）的讲解</h4>
            <div class="answer">
              <strong :data-testid="card.played ? 'review-played' : 'review-answer'">{{ card.played?.notation ?? card.answer.notation }}</strong>
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
              <n-alert v-for="w in card.basis.warnings" :key="w" type="info" :show-icon="false" class="gap">{{ w }}</n-alert>
              <p class="conditions" data-testid="review-version">
                加入复习时的讲解（讲解规则第 {{ card.basis.generator_version }} 版<template v-if="card.basis.conditions">；分析条件：{{ card.basis.conditions }}</template>）
              </p>
            </template>
            <p v-else class="muted">加入复习时这一着的讲解还没有完成<template v-if="current.source_available">，可以打开原局面查看</template>。</p>
            <h4>原来的思考</h4>
            <p data-testid="review-thinking">{{ thinkingText(card.thinking) }}</p>
            <ul v-if="hints" class="hints" data-testid="review-hints">
              <li><span class="hint-level">提示 1 · 局面需求</span>{{ hints.observe }}</li>
              <li v-if="hints.focus"><span class="hint-level">提示 2 · 相关棋子</span>{{ hints.focus.text }}</li>
              <li v-if="hints.moves.length">
                <span class="hint-level">提示 3 · 可以考虑的着法</span>{{ hints.moves.map((m) => m.notation).join("、") }}（不分先后）
              </li>
            </ul>
            <template v-if="current.themes.length">
              <h4>这一步涉及的主题</h4>
              <ul class="claims" data-testid="review-themes">
                <li v-for="t in current.themes" :key="t.theme"><strong>{{ THEME_LABEL[t.theme] }}</strong>：{{ t.why }}</li>
              </ul>
              <p class="muted">主题由程序根据棋谱中这一着的规则事实建议。</p>
            </template>
          </section>

          <!-- 下一次检查什么：学习点、自己确认的问题与理解 -->
          <section class="group">
            <h3>下一次检查什么</h3>
            <p v-if="card.basis" class="lesson"><span class="hint-level">学习点</span>{{ card.basis.lesson }}</p>
            <h4>我当时的问题（自己确认，可多选）</h4>
            <div class="issues" data-testid="review-issues">
              <n-button
                v-for="i in ISSUES"
                :key="i"
                size="tiny"
                :type="card.issues.includes(i) ? 'primary' : 'default'"
                :secondary="!card.issues.includes(i)"
                :aria-pressed="card.issues.includes(i)"
                :data-testid="`review-issue-${i}`"
                @click="toggleIssue(i)"
              >
                {{ ISSUE_LABEL[i] }}
              </n-button>
            </div>
            <h4>你的理解或疑问</h4>
            <n-input
              :value="currentNote?.text ?? ''"
              type="textarea"
              :autosize="{ minRows: 3, maxRows: 8 }"
              placeholder="写下这次复习的理解，或还没想通的地方"
              data-testid="review-note"
              @update:value="editNote"
              @blur="() => saveNote()"
            />
            <p class="muted note-state" data-testid="review-note-state">
              <template v-if="currentNote?.state === 'saving'">保存中…</template>
              <template v-else-if="currentNote?.state === 'saved'">已保存</template>
              <span v-else-if="currentNote?.state === 'failed'" class="error">保存失败：{{ currentNote.error }}（离开输入框时会再次保存）</span>
              <template v-else-if="currentNote?.state === 'editing'">有未保存的修改，离开输入框时保存。</template>
              <template v-else>加入复习时从这一步的笔记带入；离开输入框时保存。</template>
            </p>
            <p v-if="recent.length > 1" class="muted" data-testid="review-attempts">
              最近 {{ recent.length }} 次复习：<span
                v-for="(a, i) in recent"
                :key="i"
                :class="attemptMark(a).tone"
                :title="`${GOAL_LABEL[a.goal]}${a.decision?.verdict ? ` · ${decisionTitle(a.decision)}` : ''}`"
                >{{ attemptMark(a).text }}</span
              >
              <span class="legend">（复现棋谱 ✓ 一致 ✗ 不同；局面决策 ○ 比较支持这个选择 △ 不支持 · 无法判断）</span>
            </p>
            <p v-if="attempt?.goal === 'decision' && !suggestion && decision?.state !== 'pending'" class="muted" data-testid="review-self-check">
              与棋谱一致或评分接近，不等于已经理解：对照原来的意图与这一步实际的变化，说得清理由再标为“已理解”。
            </p>
          </section>

        </template>
        <n-alert v-if="review.error" type="error" :show-icon="false" class="gap">{{ review.error }}</n-alert>
      </div>
    </div>

    <!-- 当前阶段的操作 -->
    <footer class="actions" data-testid="review-actions">
      <template v-if="!revealed">
        <n-button
          v-if="card.kind === 'threat'"
          size="small"
          :type="review.marks.length ? 'primary' : 'default'"
          :disabled="review.busy || !review.marks.length"
          data-testid="review-submit-marks"
          @click="submitChoice(null)"
        >
          提交（已点 {{ review.marks.length }} 处）
        </n-button>
        <n-button size="small" data-testid="review-reveal" :disabled="review.busy" @click="submitChoice(null, true)">{{ card.kind === "threat" ? "直接看答案" : "不走子，直接看答案" }}</n-button>
        <n-button v-if="review.hints < 3" size="small" quaternary data-testid="review-hint" :disabled="review.busy" @click="showReviewHint">
          {{ review.hints ? `再看一级提示（已看 ${review.hints} 级）` : "看提示" }}
        </n-button>
        <!-- 跳过：没有作答就离开，与直接看答案分开记录 -->
        <n-button v-if="session" size="small" quaternary data-testid="review-skip" :disabled="review.busy" @click="skipCard">跳过这一题</n-button>
      </template>
      <template v-else>
        <n-button size="small" :type="primaryMark === 'again' ? 'primary' : 'default'" data-testid="review-mark-again" @click="mark('again')">需要再练</n-button>
        <n-button size="small" :type="primaryMark === 'understood' ? 'primary' : 'default'" data-testid="review-mark-understood" @click="mark('understood')">已理解</n-button>
        <n-button size="small" quaternary @click="retryCard">再试一次</n-button>
        <n-button
          size="small"
          :type="nextPrimary ? 'primary' : 'default'"
          :quaternary="!session"
          :disabled="!session && !nextCardId"
          data-testid="review-next"
          @click="showNext"
        >
          {{ nextLabel }}
        </n-button>
      </template>
    </footer>
  </section>
</template>

<style scoped>
.options {
  display: flex;
  gap: var(--sp-2);
  margin: var(--sp-2) 0;
}
.confidence {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-top: var(--sp-2);
}
.review-hints {
  margin-top: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--radius);
  background: var(--panel-soft, var(--panel));
  border: 1px solid var(--border);
}
.review-hints p {
  margin: 4px 0;
  line-height: 1.6;
}
.practice {
  min-width: 0;
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
  flex-wrap: wrap;
  gap: 4px;
}
.body {
  display: grid;
  grid-template-columns: minmax(240px, 400px) minmax(0, 1fr);
  gap: var(--sp-4);
  align-items: start;
}
/* 专注练习：棋盘更大，右侧在视口内滚动 */
.practice.session .body {
  grid-template-columns: minmax(280px, 560px) minmax(300px, 1fr);
}
.practice.session .side {
  max-height: calc(100vh - 200px);
}
@media (max-width: 960px) {
  .body,
  .practice.session .body {
    grid-template-columns: minmax(0, 1fr);
  }
  .board {
    max-width: 420px;
  }
  .side,
  .practice.session .side {
    max-height: none;
    overflow: visible;
  }
}
.side {
  min-width: 0;
  max-height: 64vh;
  overflow: auto;
  padding-right: 4px;
}
.group + .group {
  margin-top: var(--sp-3);
  padding-top: var(--sp-3);
  border-top: 1px solid var(--border);
}
.side h3 {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 6px;
  font-size: 15px;
  font-family: var(--font-serif);
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
.badge {
  font-family: var(--font-sans, inherit);
  font-size: 12px;
  font-weight: 600;
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--tag-gray-bg);
  color: var(--text-secondary);
}
.badge.ok {
  color: var(--accent-green);
}
.badge.miss {
  color: var(--accent-amber);
}
.result {
  font-weight: 600;
  color: var(--accent-amber);
}
.result.ok,
.ok {
  color: var(--accent-green);
}
.result.neutral {
  color: var(--text-primary);
}
.miss {
  color: var(--accent-amber);
}
.legend {
  font-size: 12px;
}
.goal,
.intent,
.issues {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}
.decision {
  font-weight: 600;
}
.decision.ok {
  color: var(--accent-green);
}
.decision.miss {
  color: var(--accent-amber);
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
/* 当前阶段的操作固定在可见区域底部：讲解较长、窗口较窄时也容易找到 */
.actions {
  position: sticky;
  bottom: 0;
  z-index: 1;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: var(--sp-3);
  padding: var(--sp-2) 0;
  background: var(--panel);
  box-shadow: 0 -1px 0 var(--border);
}
</style>
