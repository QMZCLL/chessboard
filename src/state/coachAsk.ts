// 教练追问：在对比面板中就当前这组对比提问。回答由后台依据对比中的程序结论生成并推送（见 xq-app llm/ask.rs），
// 按比较起点、两步着法与问题类型归属：换到别的对比或别的着法后，迟到的回答只记在原来的问题上。
import { computed, reactive, watch } from "vue";
import { api, errorMessage, on } from "../api/backend";
import type * as T from "../api/types";
import { cmp } from "./compare";
import { blockedReason, newerAnswer, questionKey, questionText, QUESTION_KINDS } from "./coachQuestions";
import { previewFrom, refreshStatus } from "./workbench";

export const ask = reactive({
  answers: {} as Record<string, T.AnswerView>,
  /** 提问没有被接受的原因，按问题 */
  errors: {} as Record<string, string>,
  sending: {} as Record<string, boolean>,
  /** 取回这组对比已有回答时的错误 */
  loadError: null as string | null,
});

let subscribed = false;

function adopt(v: T.AnswerView) {
  const key = questionKey(v.question);
  const before = ask.answers[key];
  ask.answers[key] = newerAnswer(before, v);
  // 回答完成或失败后，今天的调用次数已经变化
  const done = (s: T.CoachStatus) => s === "ready" || s === "failed";
  if (ask.answers[key] === v && done(v.status) && before && !done(before.status)) void refreshStatus();
}

function subscribe() {
  if (subscribed) return;
  subscribed = true;
  on<T.AnswerView>("coach_answer_updated", adopt);
}

/** 当前对比的两步着法 */
const pair = computed(() => {
  const v = cmp.view;
  return v ? { study_id: v.study_id, node_id: v.node_id, mine: v.mine, reference: v.reference } : null;
});

export interface QuestionItem {
  kind: T.QuestionKind;
  key: string;
  label: string;
  question: T.Question;
  view: T.AnswerView | null;
  /** 现在不能提问的原因 */
  blocked: string | null;
  error: string | null;
  sending: boolean;
}

export const questions = computed<QuestionItem[]>(() => {
  const v = cmp.view;
  const p = pair.value;
  if (!v || !p) return [];
  return QUESTION_KINDS.map((kind) => {
    const question = { ...p, kind };
    const key = questionKey(question);
    return {
      kind,
      key,
      label: questionText(kind, v.comparison.mine.notation, v.comparison.reference.notation),
      question,
      view: ask.answers[key] ?? null,
      blocked: blockedReason(v, kind),
      error: ask.errors[key] ?? null,
      sending: !!ask.sending[key],
    };
  });
});

async function loadAnswers() {
  subscribe();
  const p = pair.value;
  if (!p) return;
  try {
    for (const v of await api.coachAnswers(p.study_id, p.node_id, p.mine, p.reference)) adopt(v);
    ask.loadError = null;
  } catch (e) {
    ask.loadError = `没有取回这组对比已有的回答：${errorMessage(e)}`;
  }
}

// 打开或换了对照、对比搜索完成时，取回这组对比已有的回答（回答是否过期也随对比的依据更新）
watch(
  () => {
    const v = cmp.view;
    return v ? `${v.study_id}/${v.node_id}/${v.mine}/${v.reference}/${v.pending}/${v.seq}` : null;
  },
  (key) => {
    if (key) void loadAnswers();
  },
);

export async function askCoach(kind: T.QuestionKind) {
  subscribe();
  const p = pair.value;
  if (!p) return;
  const question = { ...p, kind };
  const key = questionKey(question);
  ask.sending[key] = true;
  delete ask.errors[key];
  try {
    adopt(await api.askCoach(question));
  } catch (e) {
    ask.errors[key] = errorMessage(e);
  } finally {
    ask.sending[key] = false;
  }
}

/** 演示回答下方的变化：从比较起点开始 */
export function demoAnswer(view: T.AnswerView, i: number) {
  const d = view.record?.result?.demos[i];
  if (!d) return;
  previewFrom({
    title: `${d.label}（从比较起点）`,
    source: `ask:${questionKey(view.question)}:${i}`,
    baseNode: view.question.node_id,
    moves: d.moves,
    notation: d.notation,
  });
}
