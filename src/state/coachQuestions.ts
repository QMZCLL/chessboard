// 教练追问的问题：哪些问题可以就这组对比提出、问题的文字、回答的新旧。只依赖数据，便于单独测试。
import type { AllowedClaim, AnswerView, CoachText, CompareView, Question, QuestionKind } from "../api/types";

export const QUESTION_KINDS: QuestionKind[] = ["my_move", "difference", "reply"];

/** 问题的文字（与后台交给模型的问题相同） */
export function questionText(kind: QuestionKind, mine: string, reference: string): string {
  if (kind === "my_move") return `我的选择${mine}起了什么作用？`;
  if (kind === "difference") return `${mine}与${reference}的差别在哪里？`;
  return `走${mine}之后，对方的回应需要怎样应对？`;
}

export function questionKey(q: Question): string {
  return `${q.study_id}/${q.node_id}/${q.mine}-${q.reference}/${q.kind}`;
}

/**
 * 这组对比现在能否提出这个问题；不能时给出原因（与后台的判断一致：只用已经完成的对比，
 * 证据中要有与问题相符的结论）。
 */
export function blockedReason(view: CompareView, kind: QuestionKind): string | null {
  if (view.pending) return "等对比搜索完成后再提问";
  // 引擎可用而搜索没有得到结果；引擎不可用时只用规则事实回答
  if (view.error && view.conditions_key) return "比较搜索没有得到结果，暂时不能提问";
  const c = view.comparison;
  if (kind === "my_move" && !c.mine.changes.length) return "这一步没有可以单独说明的直接作用";
  if (kind === "difference" && !c.differences.length) return "两步在规则事实上没有直接可比的差别，也还没有搜索结果";
  if (kind === "reply" && !c.mine.reply) return "没有能合法回放的对方回应";
  return null;
}

/** 同一个问题的两份状态中较新的一份（按状态版本，相同时采用后到的） */
export function newerAnswer(current: AnswerView | undefined, incoming: AnswerView): AnswerView {
  return current && current.seq > incoming.seq ? current : incoming;
}

/** 一段回答引用的程序结论原文 */
export function basisOf(text: CoachText, claims: AllowedClaim[]): string[] {
  return text.claim_ids.map((id) => claims.find((c) => c.id === id)?.statement).filter((s): s is string => !!s);
}
