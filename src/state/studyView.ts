// 会话视图的采用：所有视图与讲解事件都经过这里，按 viewSync.ts 单调合并，较旧的部分不覆盖较新的部分。

import { api, errorMessage } from "../api/backend";
import type * as T from "../api/types";
import { nodeOf, notify, wb } from "./core";
import { decideEvent, mergeView, newestRecord, type ViewCursor } from "./viewSync";

export { newestRecord };

/** 当前视图的讲解部分已经采用到哪里（见 viewSync.ts）：只对当前会话有意义 */
let cursor: ViewCursor | null = null;

/**
 * 讲解事件：较旧的事件（这一着已经采用了更新的消息）不改动这一着的讲解状态，统计只采用更新的；
 * 记录按保存次数比较，与事件到达的先后无关。来自新事件流的事件（后台重新启动）之后重新读取一次视图。
 */
export function onExplanation(p: T.ExplanationEvent) {
  if (!wb.view || wb.view.study.id !== p.study_id || !cursor) return;
  const d = decideEvent(cursor, p);
  cursor = d.cursor;
  if (d.node) wb.view.explain_status[String(p.node_id)] = p.status;
  if (d.stats) wb.view.stats = p.stats;
  if (d.resync) requestRefresh(p.study_id);
  wb.records[p.node_id] = newestRecord(wb.records[p.node_id], p.record)!;
  if (wb.detail && wb.detail.study_id === p.study_id && wb.detail.node.id === p.node_id) {
    wb.detail.explanation = newestRecord(wb.detail.explanation, p.record);
  }
}

/**
 * 采用后台返回的会话视图：所有视图都经过这里，按棋谱树与讲解部分分别合并（见 viewSync.ts），
 * 较旧的部分不覆盖较新的部分，界面不会短暂回退。open 为打开会话（可以换成另一个会话）；
 * 其他来源（命令的响应、重新读取）属于之前的会话时不采用，不会把界面切回那个会话。
 */
/** 选中的着法不在新的棋谱树中时改选哪一着（由 navigation.ts 接上 selectNode） */
let repairSelection: (id: number) => void = () => undefined;
export function setSelectionRepair(fn: (id: number) => void) {
  repairSelection = fn;
}

/** 选中的着法与它的各级上一着（从近到远）：棋谱树改变后，选中的着法被删除时改选还在的最近一着 */
function selectionPath(): number[] {
  const path: number[] = [];
  for (let cur = nodeOf(wb.selected); cur; cur = cur.parent === null ? null : nodeOf(cur.parent)) path.push(cur.id);
  return path;
}

export function setView(view: T.StudyView, open = false) {
  if (!open && wb.view?.study.id !== view.study.id) return;
  const sameStudy = wb.view?.study.id === view.study.id;
  const path = sameStudy ? selectionPath() : [];
  if (!wb.view || wb.view.study.id !== view.study.id) {
    wb.revealed = {};
    wb.attempts = {};
    wb.records = {};
    wb.coach = {};
    wb.analysis = null;
    // 节点编号只在同一个会话内有意义
    wb.trial = null;
    wb.collapsed = {};
    wb.thinkSession = null;
    wb.thinkBase = null;
  }
  if (applyView(view)) requestRefresh(view.study.id);
  // 每次采用之后检查选中的着法还在不在（例如别处删除了这一着）
  if (sameStudy && !nodeOf(wb.selected)) {
    const keep = path.find((id) => nodeOf(id));
    repairSelection(keep ?? 0);
  }
}

/** 合并一份视图，返回是否需要再读取一份完整的视图（棋谱树与讲解部分来自不同的时刻） */
function applyView(view: T.StudyView): boolean {
  const r = mergeView(wb.view, cursor, view);
  cursor = r.cursor;
  if (!r.ignored && r.view !== wb.view) wb.view = r.view;
  return r.refresh;
}

let refreshing = false;
let refreshAgain = false;

/**
 * 再读取一份完整的视图，使统计与棋谱树一致；同一时间只有一个读取，读取期间又有需要时读完再读一次。
 * 读到的视图同样经过合并，不会让界面回退；正确性不依赖这次读取（读取失败或一直没有返回时，界面保持合并后的状态）。
 */
function requestRefresh(id: string) {
  if (refreshing) {
    refreshAgain = true;
    return;
  }
  refreshing = true;
  void (async () => {
    try {
      do {
        refreshAgain = false;
        const fresh = await api.studyView(id);
        if (wb.view?.study.id !== id) return;
        applyView(fresh);
      } while (refreshAgain);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      refreshing = false;
    }
  })();
}
