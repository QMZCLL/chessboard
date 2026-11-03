// 复盘收件箱：一次导入多盘自己下的棋，引擎空闲时逐盘分析，分析完成后一次出题（见 xq-app inbox.rs）。
// 列表在打开首页时读取；收件箱有变化（inbox_updated）或收件箱中还没有分析完的棋局有讲解更新时刷新：
// 一段时间内的多次事件合并为一次读取；读取期间又要读取时，读完之后再读一次，不会漏掉最后的进度。
import { reactive } from "vue";
import { api, errorMessage, on } from "../api/backend";
import type * as T from "../api/types";
import { loadReviews } from "./review";

export const inbox = reactive({
  list: null as T.InboxList | null,
  error: null as string | null,
  /** 正在出题 */
  generating: false,
  /** 最近一次出题的结果 */
  generated: null as string | null,
});

/** 讲解事件合并读取的间隔 */
const REFRESH_MS = 1000;
let timer: ReturnType<typeof setTimeout> | null = null;
let current: Promise<void> | null = null;
let queued: Promise<void> | null = null;

async function fetchList() {
  try {
    inbox.list = await api.inboxList();
    inbox.error = null;
  } catch (e) {
    inbox.error = errorMessage(e);
  }
}

/**
 * 读取收件箱。正在读取时，等这次读完再读一次（读取期间可能又有了变化），同时的多次调用共用这一次；
 * 返回时列表已经是调用之后读到的。
 */
export function loadInbox(): Promise<void> {
  watchInbox();
  if (!current) {
    current = fetchList().finally(() => (current = null));
    return current;
  }
  queued ??= current.then(() => {
    queued = null;
    return loadInbox();
  });
  return queued;
}

function scheduleRefresh() {
  if (timer) return;
  timer = setTimeout(() => {
    timer = null;
    void loadInbox();
  }, REFRESH_MS);
}

let subscribed = false;
function watchInbox() {
  if (subscribed) return;
  subscribed = true;
  on("inbox_updated", () => scheduleRefresh());
  on<{ study_id?: string }>("explanation_updated", (e) => {
    if (inbox.list?.items.some((i) => i.study_id === e.study_id && !i.done)) scheduleRefresh();
  });
}

/** 已经分析完、还没有出过题的盘数 */
export const readyCount = (list: T.InboxList | null) => list?.items.filter((i) => i.done && !i.exercises).length ?? 0;

/** 为已经分析完、还没有出过题的棋局出题；新增的题目进入复习库 */
export async function generateInbox(): Promise<boolean> {
  inbox.generating = true;
  try {
    const r = await api.inboxGenerate();
    // 列表读回之后再显示结果：结果与每盘的出题情况同时出现
    await Promise.all([loadInbox(), loadReviews()]);
    inbox.generated = r.text;
    return true;
  } catch (e) {
    inbox.error = errorMessage(e);
    return false;
  } finally {
    inbox.generating = false;
  }
}

/** 移出收件箱：棋局与题目都保留 */
export async function removeFromInbox(studyId: string) {
  try {
    inbox.list = await api.inboxRemove(studyId);
    inbox.error = null;
  } catch (e) {
    inbox.error = errorMessage(e);
  }
}

/** 一盘的进度说明 */
export function progressText(i: T.InboxItem): string {
  if (!i.done) return `分析中 ${i.analysed} / ${i.moves} 着`;
  const parts = [`已分析 ${i.moves} 着`];
  if (i.incomplete) parts.push(`其中 ${i.incomplete} 着分析不完整`);
  if (i.key_positions !== null) parts.push(i.key_positions ? `关键局面 ${i.key_positions} 处` : "没有选出关键局面");
  return parts.join(" · ");
}

/** 设置中的“我的名字”：逗号、顿号、分号或换行分开（名字中间可以有空格） */
export function parseNames(text: string): string[] {
  return text
    .split(/[,，、;；\n]+/)
    .map((n) => n.trim())
    .filter(Boolean);
}
