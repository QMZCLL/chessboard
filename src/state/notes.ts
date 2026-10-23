// 笔记草稿：按（会话, 着法）保存在工作台状态中，不放在编辑器组件里。
// - 布局跨断点时讲解面板会重建，草稿与保存状态不随组件销毁；
// - 编辑器显示的是草稿（有未保存的修改时）或棋谱中已保存的内容，不保留一份可能过期的副本；
// - 只保存用户修改过的内容：没有草稿就不写入，不会把旧值写回；
// - 同一着的保存依次进行，保存期间又有修改时，完成后再保存一次。
import { reactive, watch } from "vue";
import { errorMessage } from "../api/backend";
import { nodeOf, saveNote, study, wb } from "./workbench";

interface Draft {
  studyId: string;
  nodeId: number;
  text: string;
  starred: boolean;
  /** 每次修改递增；保存完成时据此判断保存期间是否又有修改 */
  rev: number;
  saving: boolean;
  /** 保存期间又请求了保存 */
  again: boolean;
  failed: string | null;
}

export type NoteState = "idle" | "editing" | "saving" | "saved" | "failed";

const drafts = reactive<Record<string, Draft>>({});
/** 最近保存成功、之后没有再修改的笔记 */
const saved = reactive<Record<string, true>>({});

const keyOf = (studyId: string, nodeId: number) => `${studyId}:${nodeId}`;

/** 某一着的笔记在界面上的样子：有草稿时显示草稿，否则显示棋谱中已保存的内容。 */
export function noteView(studyId: string, nodeId: number) {
  const key = keyOf(studyId, nodeId);
  const d = drafts[key];
  if (d) {
    const state: NoteState = d.saving ? "saving" : d.failed ? "failed" : "editing";
    return { text: d.text, starred: d.starred, state, error: d.failed };
  }
  const node = study.value?.id === studyId ? nodeOf(nodeId) : undefined;
  const state: NoteState = saved[key] ? "saved" : "idle";
  return { text: node?.note ?? "", starred: node?.starred ?? false, state, error: null };
}

/** 用户修改笔记：建立或更新草稿。 */
export function editNote(studyId: string, nodeId: number, patch: { text?: string; starred?: boolean }) {
  const key = keyOf(studyId, nodeId);
  let d = drafts[key];
  if (!d) {
    const current = noteView(studyId, nodeId);
    d = { studyId, nodeId, text: current.text, starred: current.starred, rev: 0, saving: false, again: false, failed: null };
    drafts[key] = d;
  }
  if (patch.text !== undefined) d.text = patch.text;
  if (patch.starred !== undefined) d.starred = patch.starred;
  d.rev += 1;
  delete saved[key];
}

/** 保存某一着的草稿；没有草稿（没有修改）时什么也不做。 */
export async function persistNote(studyId: string, nodeId: number): Promise<void> {
  const key = keyOf(studyId, nodeId);
  const d = drafts[key];
  if (!d) return;
  if (d.saving) {
    d.again = true;
    return;
  }
  const rev = d.rev;
  d.saving = true;
  d.failed = null;
  try {
    await saveNote(studyId, nodeId, d.text, d.starred);
    // 保存期间没有新的修改：草稿已经写入棋谱，之后显示棋谱中的内容
    if (drafts[key] === d && d.rev === rev) {
      delete drafts[key];
      saved[key] = true;
    }
  } catch (e) {
    d.failed = errorMessage(e);
  } finally {
    d.saving = false;
    if (d.again && drafts[key] === d) {
      d.again = false;
      void persistNote(studyId, nodeId);
    }
  }
}

// 离开某一着（包括程序切换选中的着法，例如屏幕连接跟随最新局面）时，保存它未保存的修改
watch(
  () => (study.value ? keyOf(study.value.id, wb.selected) : null),
  (_, previous) => {
    const d = previous ? drafts[previous] : undefined;
    if (d) void persistNote(d.studyId, d.nodeId);
  },
);
