// 会话视图的单调合并。后台的视图快照（命令的响应、主动读取、study_updated 之后的读取）与讲解事件可能以任意顺序到达，
// 所有写入都经过这里：较旧的部分不会覆盖较新的部分，界面不会出现短暂的回退。只依赖数据，便于单独测试。
//
// 版本（都只在同一个事件流 stream_id 中可以比较，后台重新启动后从头开始）：
// - 视图序号 view_seq：棋谱或任何一着的讲解写入之后加一。后台生成视图时先取它，序号为 v 的视图反映了序号不超过 v 的
//   全部变化；它不小于已经采用的（视图或事件）时，这份视图整体新于界面，直接采用。
// - 比不出先后时（例如传入的视图有更新的棋谱树、却早于已经收到的讲解事件），按两部分分别比较：
//   棋谱树按 study.revision（保存在数据库中）；讲解部分（各着的讲解状态与统计）按讲解序号 explain_seq
//   （后台每发出一个讲解事件加一，序号为 s 的视图已经反映了序号不超过 s 的全部讲解事件）。
import type { ExplanationEvent, ExplanationRecord, StudyView } from "../api/types";

/** 当前视图的讲解部分已经采用到哪里 */
export interface ViewCursor {
  /** 事件流：后台重新启动后不同，序号不再可比 */
  stream: string;
  /** 已经采用的最大视图序号（视图或事件） */
  view: number;
  /** 采用的视图快照的序号：没有单独记下的着法都反映到这里 */
  base: number;
  /** 快照之后各着采用的讲解事件的序号 */
  nodes: Record<string, number>;
  /** 统计来自的序号 */
  stats: number;
}

export const cursorOf = (view: StudyView): ViewCursor => ({
  stream: view.stream_id,
  view: view.view_seq,
  base: view.explain_seq,
  nodes: {},
  stats: view.explain_seq,
});

/** 讲解部分已经采用的最大序号 */
export const appliedSeq = (c: ViewCursor): number => Math.max(c.base, c.stats, ...Object.values(c.nodes));

export interface MergeResult {
  view: StudyView;
  cursor: ViewCursor;
  /** 没有整体采用这份视图（棋谱树与讲解部分来自不同的时刻）：需要再读取一份完整的视图，统计才与棋谱树一致 */
  refresh: boolean;
  /** 这份视图完全早于当前视图，什么都没有改变 */
  ignored: boolean;
}

/**
 * 合并一份视图。不同的会话、还没有视图或事件流已经改变（序号不可比）时整体采用；
 * 同一个会话、同一个事件流中，视图序号不小于已经采用的时整体采用；否则两部分分别比较：
 * - 棋谱树：revision 不小于当前的才采用；
 * - 讲解部分：序号不小于已经采用的（视图或事件）时采用这份视图的讲解状态与统计，当前视图中有而它没有的着法保留；
 *   更旧时保留当前的讲解状态与统计，只补上当前没有的着法；
 * - 两部分都更旧时什么都不改变；两部分都不更旧时整体采用（已经删除的着法的讲解状态一并去掉）。
 */
export function mergeView(current: StudyView | null, cursor: ViewCursor | null, incoming: StudyView): MergeResult {
  if (!current || !cursor || current.study.id !== incoming.study.id || cursor.stream !== incoming.stream_id) {
    return { view: incoming, cursor: cursorOf(incoming), refresh: false, ignored: false };
  }
  if (incoming.view_seq >= cursor.view) {
    // 它反映了已经采用的全部变化：之前采用的讲解事件即使序号大于它的讲解序号（后台在取两个序号之间写入了记录），
    // 也已经在其中，之后不小于这个序号的才是更新的消息
    const covered = Math.max(incoming.explain_seq, appliedSeq(cursor));
    return { view: incoming, cursor: { ...cursorOf(incoming), base: covered, stats: covered }, refresh: false, ignored: false };
  }
  const treeNewer = incoming.study.revision >= current.study.revision;
  const explainNewer = incoming.explain_seq >= appliedSeq(cursor);
  if (treeNewer && explainNewer) return { view: incoming, cursor: cursorOf(incoming), refresh: false, ignored: false };
  if (!treeNewer && !explainNewer) return { view: current, cursor, refresh: false, ignored: true };
  const study = treeNewer ? incoming.study : current.study;
  if (explainNewer) {
    // 传入的棋谱树更旧：它没有的着法（之后才加入的）保留当前的讲解状态
    return {
      view: { ...incoming, study, explain_status: { ...current.explain_status, ...incoming.explain_status }, view_seq: cursor.view },
      cursor: { ...cursorOf(incoming), view: cursor.view },
      refresh: true,
      ignored: false,
    };
  }
  // 传入的讲解部分更旧：保留当前的讲解状态与统计，只补上当前没有的着法
  return {
    view: {
      ...current,
      study,
      explain_status: { ...incoming.explain_status, ...current.explain_status },
    },
    cursor,
    refresh: true,
    ignored: false,
  };
}

export interface EventDecision {
  /** 采用这个事件中这一着的讲解状态与记录 */
  node: boolean;
  /** 采用这个事件中的统计 */
  stats: boolean;
  cursor: ViewCursor;
  /** 事件来自新的事件流（后台重新启动）：序号不再可比，需要重新读取一次完整的视图 */
  resync: boolean;
}

/**
 * 一个讲解事件能否采用。同一个事件流中，这一着最后采用的序号（或视图快照的序号）不小于它时不改动这一着，
 * 统计只采用更新的；重复到达的事件没有作用。来自新事件流的事件是这一着最新的消息，采用它并重新读取一次视图。
 */
export function decideEvent(cursor: ViewCursor, ev: Pick<ExplanationEvent, "seq" | "stream_id" | "node_id" | "view_seq">): EventDecision {
  const key = String(ev.node_id);
  if (ev.stream_id !== cursor.stream) {
    return {
      node: true,
      stats: true,
      cursor: { stream: ev.stream_id, view: ev.view_seq, base: 0, nodes: { [key]: ev.seq }, stats: ev.seq },
      resync: true,
    };
  }
  const node = ev.seq > (cursor.nodes[key] ?? cursor.base);
  const stats = ev.seq > cursor.stats;
  if (!node && !stats) return { node, stats, cursor, resync: false };
  return {
    node,
    stats,
    cursor: {
      ...cursor,
      view: Math.max(cursor.view, ev.view_seq),
      nodes: node ? { ...cursor.nodes, [key]: ev.seq } : cursor.nodes,
      stats: stats ? ev.seq : cursor.stats,
    },
    resync: false,
  };
}

/** 两条讲解记录中较新的一条：按保存次数比较，次数相同（或早期的记录）时按保存时间，再相同时保留已有的一条。 */
export function newestRecord(
  a: ExplanationRecord | null | undefined,
  b: ExplanationRecord | null | undefined,
): ExplanationRecord | null {
  if (!a) return b ?? null;
  if (!b) return a;
  const ra = a.revision ?? 0;
  const rb = b.revision ?? 0;
  if (rb !== ra) return rb > ra ? b : a;
  return b.updated_at > a.updated_at ? b : a;
}
