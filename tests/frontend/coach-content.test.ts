// AI 讲解文字的组织：每段带上它引用的程序结论原文与证据；学习要点为空时交给规则讲解。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { CoachResult } from "../../src/api/types.ts";
import { coachLesson, coachSections, paragraphOf } from "../../src/state/coachContent.ts";

function result(lesson: string): CoachResult {
  return {
    output: {
      summary: { text: "摘要", claim_ids: ["c1"] },
      sections: [
        { kind: "reason", text: "炮到中路施加压力。", claim_ids: ["c1", "c2"] },
        { kind: "reply", text: "留意对方出马。", claim_ids: ["c9"] },
      ],
      lesson: { text: lesson, claim_ids: ["c2"] },
      followups: [],
    },
    claims: [
      { id: "c1", kind: "fact", group: "basis", statement: "中炮攻击中卒", evidence_ids: ["e1", "e2"], certainty: "certain" },
      { id: "c2", kind: "fact", group: "lesson", statement: "检查新增的攻击关系", evidence_ids: ["e2"], certainty: "certain" },
    ],
  } as unknown as CoachResult;
}

test("段落带上引用结论的原文与去重后的证据", () => {
  const r = result("出子时检查攻击关系。");
  const p = paragraphOf(r, r.output.sections[0]);
  assert.deepEqual(p.basis, ["中炮攻击中卒", "检查新增的攻击关系"]);
  assert.deepEqual(p.claim.evidence, ["e1", "e2"]);
});

test("正文各段带标题，引用不存在的结论时不显示依据", () => {
  const sections = coachSections(result(""));
  assert.deepEqual(sections.map((s) => s.title), ["为什么这样走", "下一步注意"]);
  assert.deepEqual(sections[1].basis, []);
});

test("AI 学习要点为空时返回 null，由规则讲解的学习点承担", () => {
  assert.equal(coachLesson(result("  ")), null);
  assert.equal(coachLesson(null), null);
  assert.equal(coachLesson(result("出子时检查攻击关系。"))?.basis[0], "检查新增的攻击关系");
});
