// 会话视图的单调合并：视图快照与讲解事件以任意顺序到达时，棋谱树的版本、各着的讲解状态与统计都不回退，
// 全部到达后与最新的状态一致。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { ExplainStatus, ExplanationEvent, ExplanationRecord, StudyView } from "../../src/api/types.ts";
import { appliedSeq, cursorOf, decideEvent, mergeView, newestRecord, type ViewCursor } from "../../src/state/viewSync.ts";

const STREAM = "s1";

/** 只有比较用到的字段：棋谱树的节点、版本，讲解状态、统计、讲解序号与视图序号 */
function view(
  revision: number,
  seq: number,
  nodes: number[],
  status: Record<number, ExplainStatus>,
  detailed: number,
  viewSeq: number,
  stream = STREAM,
): StudyView {
  return {
    study: { id: "a", revision, nodes: Object.fromEntries(nodes.map((n) => [String(n), { id: n }])) } as unknown as StudyView["study"],
    explain_status: Object.fromEntries(Object.entries(status)),
    stats: { moves: nodes.length - 1, pending: 0, basic: 0, detailed, partial: 0, failed: 0 },
    explain_seq: seq,
    stream_id: stream,
    view_seq: viewSeq,
  };
}

function event(seq: number, node: number, status: ExplainStatus, detailed: number, viewSeq: number, stream = STREAM): ExplanationEvent {
  return {
    seq,
    stream_id: stream,
    view_seq: viewSeq,
    study_id: "a",
    node_id: node,
    status,
    record: { study_id: "a", node_id: node, revision: seq } as ExplanationRecord,
    stats: { moves: 0, pending: 0, basic: 0, detailed, partial: 0, failed: 0 },
  };
}

/** 与界面相同的处理：视图经过 mergeView，事件经过 decideEvent */
class Client {
  view: StudyView | null = null;
  cursor: ViewCursor | null = null;
  refreshes = 0;
  take(v: StudyView) {
    const r = mergeView(this.view, this.cursor, v);
    this.view = r.view;
    this.cursor = r.cursor;
    if (r.refresh) this.refreshes += 1;
  }
  hear(e: ExplanationEvent) {
    if (!this.view || !this.cursor) return;
    const d = decideEvent(this.cursor, e);
    this.cursor = d.cursor;
    if (d.node) this.view = { ...this.view, explain_status: { ...this.view.explain_status, [String(e.node_id)]: e.status } };
    if (d.stats) this.view = { ...this.view, stats: e.stats };
    if (d.resync) this.refreshes += 1;
  }
}

test("两个版本分开比较：更新的棋谱树与更旧的讲解状态只采用棋谱树", () => {
  const c = new Client();
  c.take(view(1, 0, [0], {}, 0, 0));
  // 讲解事件先到：新加入的一着已经讲解完成（棋谱树中还没有这一着）
  c.hear(event(5, 1, "detailed", 1, 6));
  // 走子的响应晚到：棋谱树更新，讲解状态更旧（视图序号也更旧，比不出整体先后）
  c.take(view(2, 3, [0, 1], { 1: "queued" }, 0, 4));
  assert.equal(c.view!.study.revision, 2, "采用更新的棋谱树");
  assert.equal(c.view!.explain_status["1"], "detailed", "不回退到排队中");
  assert.equal(c.view!.stats.detailed, 1, "统计不回退");
  assert.equal(c.refreshes, 1, "再读取一份完整的视图");
  // 再读取的视图整体采用
  c.take(view(2, 5, [0, 1], { 1: "detailed" }, 1, 6));
  assert.equal(c.refreshes, 1);
  assert.deepEqual(c.cursor, cursorOf(c.view!));
});

test("棋谱树按 revision 不回退；两部分都更旧的视图什么都不改变", () => {
  const c = new Client();
  c.take(view(3, 4, [0, 1, 2], { 1: "detailed", 2: "detailed" }, 2, 10));
  const before = c.view;
  c.take(view(2, 2, [0, 1], { 1: "basic" }, 0, 5));
  assert.equal(c.view, before, "完全更旧的视图被忽略");
  // 棋谱树更旧、讲解部分更新：讲解采用新的，棋谱树保留
  c.take(view(2, 6, [0, 1], { 1: "partial" }, 1, 8));
  assert.equal(c.view!.study.revision, 3);
  assert.equal(c.view!.explain_status["1"], "partial");
  assert.equal(c.view!.explain_status["2"], "detailed", "它没有的着法保留当前的讲解状态");
});

test("讲解事件：较旧与重复的事件不改动这一着与统计；不同着法各自比较", () => {
  const c = new Client();
  c.take(view(1, 10, [0, 1, 2], { 1: "basic", 2: "basic" }, 0, 20));
  c.hear(event(12, 1, "detailed", 1, 22));
  c.hear(event(11, 1, "basic", 0, 21));
  assert.equal(c.view!.explain_status["1"], "detailed", "seq 12 之后的 seq 11 不改动这一着");
  assert.equal(c.view!.stats.detailed, 1, "统计不回退");
  c.hear(event(12, 1, "detailed", 1, 22));
  assert.equal(appliedSeq(c.cursor!), 12, "重复的事件没有作用");
  // 另一着的事件序号较小，但它是这一着最新的消息
  c.hear(event(11, 2, "partial", 0, 21));
  assert.equal(c.view!.explain_status["2"], "partial");
  assert.equal(c.view!.stats.detailed, 1, "统计仍按序号");
  // 不超过视图快照序号的事件已经反映在视图中
  c.hear(event(9, 2, "queued", 0, 19));
  assert.equal(c.view!.explain_status["2"], "partial");
});

test("事件流改变：序号不再可比，采用新的事件或视图并重新读取一次", () => {
  const c = new Client();
  c.take(view(4, 100, [0, 1], { 1: "basic" }, 0, 200));
  c.hear(event(2, 1, "detailed", 1, 3, "s2"));
  assert.equal(c.view!.explain_status["1"], "detailed", "新事件流中较小的序号照样采用");
  assert.equal(c.refreshes, 1);
  assert.equal(c.cursor!.stream, "s2");
  c.take(view(4, 3, [0, 1], { 1: "detailed" }, 1, 4, "s2"));
  assert.equal(c.view!.explain_seq, 3);
  // 后台又重新启动：新事件流的视图整体采用
  c.take(view(4, 1, [0, 1], { 1: "queued" }, 0, 1, "s3"));
  assert.equal(c.cursor!.stream, "s3");
});

test("视图序号不小于已经采用的：整体采用，即使讲解序号较小", () => {
  // 后台先取视图序号再取讲解序号：两次之间写入的一着已经在记录中，讲解序号却还没有加一
  const c = new Client();
  c.take(view(1, 5, [0, 1], { 1: "basic" }, 0, 10));
  c.hear(event(6, 1, "detailed", 1, 12));
  c.take(view(1, 5, [0, 1], { 1: "detailed" }, 1, 12));
  assert.equal(c.refreshes, 0, "整体采用，不需要再读取");
  assert.deepEqual([c.view!.explain_status["1"], c.cursor!.view], ["detailed", 12]);
  // 视图序号更旧时仍按两部分比较
  c.take(view(1, 5, [0, 1], { 1: "basic" }, 0, 11));
  assert.equal(c.view!.explain_status["1"], "detailed");
});

test("讲解记录按保存次数比较，不按保存时间", () => {
  const rec = (revision: number, updated_at: number) => ({ revision, updated_at }) as ExplanationRecord;
  assert.equal(newestRecord(rec(3, 100), rec(2, 200))?.revision, 3, "墙钟时间更晚但保存次数更少的是旧的");
  assert.equal(newestRecord(rec(2, 100), rec(3, 100))?.revision, 3, "同一毫秒内的两次保存也分得出先后");
  const kept = rec(2, 100);
  assert.equal(newestRecord(kept, rec(2, 100)), kept, "相同时保留已有的");
  assert.equal(newestRecord(null, kept), kept);
});

/** 全部排列 */
function* permutations<T>(items: T[]): Generator<T[]> {
  if (items.length <= 1) return yield items;
  for (let i = 0; i < items.length; i++) {
    for (const rest of permutations([...items.slice(0, i), ...items.slice(i + 1)])) yield [items[i], ...rest];
  }
}

test("视图与事件以任意顺序到达：每一步都不回退，全部到达后与最新的状态一致", () => {
  // 后台的经过（括号中是讲解序号 / 视图序号）：走出第 1 着（棋谱树 1，-/1）→ 排队（1/2）→ 讲解完成（2/3）
  // → 走出第 2 着（棋谱树 2，-/4）→ 排队（3/5）→ 讲解完成（4/6）；三份视图分别在第 2、4、6 次变化之后读取
  const RANK: Record<ExplainStatus, number> = { queued: 0, basic: 1, partial: 2, detailed: 3, failed: 3 };
  const items: ({ v: StudyView } | { e: ExplanationEvent })[] = [
    { v: view(1, 1, [0, 1], { 1: "queued" }, 0, 2) },
    { e: event(2, 1, "detailed", 1, 3) },
    { v: view(2, 2, [0, 1, 2], { 1: "detailed" }, 1, 4) },
    { e: event(3, 2, "queued", 1, 5) },
    { e: event(4, 2, "detailed", 2, 6) },
    { v: view(2, 4, [0, 1, 2], { 1: "detailed", 2: "detailed" }, 2, 6) },
  ];
  let orders = 0;
  for (const order of permutations(items)) {
    orders += 1;
    const c = new Client();
    c.take(view(0, 0, [0], {}, 0, 0));
    let rev = 0;
    let detailed = 0;
    const ranks: Record<string, number> = {};
    for (const item of order) {
      if ("v" in item) c.take(item.v);
      else c.hear(item.e);
      const v = c.view!;
      assert.ok(v.study.revision >= rev, "棋谱树不回退");
      assert.ok(v.stats.detailed >= detailed, "统计不回退");
      for (const [node, st] of Object.entries(v.explain_status)) {
        assert.ok(RANK[st] >= (ranks[node] ?? -1), `第 ${node} 着的讲解状态不回退`);
        ranks[node] = RANK[st];
      }
      rev = v.study.revision;
      detailed = v.stats.detailed;
    }
    const v = c.view!;
    assert.deepEqual([v.study.revision, v.explain_status["1"], v.explain_status["2"], v.stats.detailed], [2, "detailed", "detailed", 2]);
  }
  assert.equal(orders, 720);
});
