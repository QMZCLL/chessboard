// AI 讲解结果中的文字与程序依据：讲解面板与 AI 讲解卡片共用，保证同一段文字只按一种方式组织。
// 模型只组织文字；段落下方的“依据”是程序给出的结论原文（评分、排名、评价与杀棋结论只出现在这里）。
import type { AllowedClaim, Claim, CoachResult, CoachText, SectionKind } from "../api/types";

export const SECTION_TITLES: Record<SectionKind, string> = {
  purpose: "为什么考虑这步",
  reason: "为什么这样走",
  comparison: "换一种走法",
  reply: "下一步注意",
  caution: "风险提示",
};

export interface CoachParagraph {
  claim: Claim;
  basis: string[];
}

/** 一段 AI 文字对应的讲解要点：文字与它引用的程序结论所带的证据。 */
export function paragraphOf(result: CoachResult, t: CoachText): CoachParagraph {
  const used = t.claim_ids.map((id) => result.claims.find((c) => c.id === id)).filter((c): c is AllowedClaim => !!c);
  const evidence = [...new Set(used.flatMap((c) => c.evidence_ids))];
  return { claim: { text: t.text, evidence }, basis: used.map((c) => c.statement) };
}

/** 正文各段（不含一句话摘要与学习要点：摘要由程序的一句话要点承担，学习要点放在学习点中）。 */
export function coachSections(result: CoachResult): (CoachParagraph & { title: string })[] {
  return result.output.sections.map((s) => ({ title: SECTION_TITLES[s.kind] ?? "", ...paragraphOf(result, s) }));
}

/** AI 的学习要点；模型没有给出时为 null，学习点使用规则讲解的内容。 */
export function coachLesson(result: CoachResult | null | undefined): CoachParagraph | null {
  if (!result || !result.output.lesson.text.trim()) return null;
  return paragraphOf(result, result.output.lesson);
}
