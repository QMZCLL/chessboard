// 教练追问：哪些问题现在可以提出（与后台的判断一致）、问题文字与归属、回答的新旧与依据原文。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { AllowedClaim, AnswerView, CompareView } from "../../src/api/types.ts";
import { basisOf, blockedReason, newerAnswer, questionKey, questionText } from "../../src/state/coachQuestions.ts";

function compare(patch: { pending?: boolean; error?: string | null; conditions_key?: string | null; changes?: number; differences?: number; reply?: boolean } = {}): CompareView {
  const claim = { text: "事实", evidence: [] };
  return {
    seq: 1,
    conditions_key: patch.conditions_key === undefined ? "cond" : patch.conditions_key,
    study_id: "s",
    node_id: 0,
    mine: "h2e2",
    reference: "h0g2",
    pending: patch.pending ?? false,
    error: patch.error ?? null,
    comparison: {
      mine: { notation: "炮二平五", changes: Array(patch.changes ?? 1).fill(claim), reply: patch.reply === false ? null : claim },
      reference: { notation: "马二进三", changes: [claim], reply: null },
      differences: Array(patch.differences ?? 1).fill(claim),
    },
  } as unknown as CompareView;
}

test("问题的文字与归属：比较起点、两步着法与问题类型", () => {
  assert.equal(questionText("my_move", "炮二平五", "马二进三"), "我的选择炮二平五起了什么作用？");
  assert.equal(questionText("difference", "炮二平五", "马二进三"), "炮二平五与马二进三的差别在哪里？");
  assert.equal(questionText("reply", "炮二平五", "马二进三"), "走炮二平五之后，对方的回应需要怎样应对？");
  const q = { study_id: "s", node_id: 3, mine: "h2e2", reference: "h0g2", kind: "reply" as const };
  assert.equal(questionKey(q), "s/3/h2e2-h0g2/reply");
  assert.notEqual(questionKey(q), questionKey({ ...q, reference: "b0c2" }));
});

test("只就已经完成的对比提问，证据中要有与问题相符的结论", () => {
  assert.equal(blockedReason(compare(), "my_move"), null);
  assert.equal(blockedReason(compare(), "reply"), null);
  assert.match(blockedReason(compare({ pending: true }), "my_move") ?? "", /完成后再提问/);
  // 搜索失败时不能提问；引擎不可用（没有搜索条件）时只用规则事实回答
  assert.match(blockedReason(compare({ error: "引擎出错" }), "my_move") ?? "", /没有得到结果/);
  assert.equal(blockedReason(compare({ error: "引擎不可用", conditions_key: null }), "my_move"), null);
  assert.match(blockedReason(compare({ reply: false }), "reply") ?? "", /对方回应/);
  assert.match(blockedReason(compare({ differences: 0 }), "difference") ?? "", /没有直接可比的差别/);
  assert.match(blockedReason(compare({ changes: 0 }), "my_move") ?? "", /直接作用/);
});

test("回答按状态版本取较新的一份，依据按引用的编号取原文", () => {
  const view = (seq: number, status: string) => ({ seq, status }) as unknown as AnswerView;
  const ready = view(5, "ready");
  assert.equal(newerAnswer(ready, view(4, "queued")), ready);
  assert.equal(newerAnswer(view(4, "queued"), ready), ready);
  assert.equal(newerAnswer(undefined, ready), ready);
  const claims = [{ id: "c1", statement: "炮二平五：架中炮" }, { id: "c2", statement: "同一次搜索中……" }] as AllowedClaim[];
  assert.deepEqual(basisOf({ text: "回答", claim_ids: ["c2", "c9", "c1"] }, claims), ["同一次搜索中……", "炮二平五：架中炮"]);
});
