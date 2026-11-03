// 学习会话：打开、新建、导入、导出、改名与翻转，以及别处修改了会话（study_updated）之后读取新的视图。

import { api, errorMessage, isTauri, pickOpenPath, pickSavePath } from "../api/backend";
import type * as T from "../api/types";
import { lastMainline, nodeOf, notify, safeStorage, study, wb } from "./core";
import { selectNode } from "./navigation";
import { setView } from "./studyView";

export async function onStudyUpdated(p: T.StudyUpdatedEvent) {
  if (!wb.view || wb.view.study.id !== p.study_id || p.revision <= wb.view.study.revision) return;
  const previousLive = wb.view.study.live_node;
  try {
    // 几次读取可能乱序返回：合并时较旧的棋谱树不会覆盖较新的
    setView(await api.studyView(p.study_id));
  } catch (e) {
    notify("error", errorMessage(e));
    return;
  }
  if (wb.view?.study.id !== p.study_id) return;
  // 用户停留在实际对局最新节点时才跟随；正在阅读其他节点时不切走
  const live = wb.view.study.live_node;
  if (wb.followLive && live !== null && live !== previousLive && (previousLive === null || wb.selected === previousLive)) {
    await selectNode(live);
  } else if (!nodeOf(wb.selected)) {
    await selectNode(0);
  }
}

export async function openStudy(id: string, select?: number) {
  const view = await api.loadStudy(id);
  setView(view, true);
  wb.main = "board";
  safeStorage("set", "xq-last-study", id);
  // 指定的着法已经删除时（例如记住的上次位置），打开默认的位置
  const wanted = select !== undefined && view.study.nodes[select] ? select : undefined;
  await selectNode(wanted ?? view.study.live_node ?? lastMainline(view));
}

export async function createStudy(req: T.CreateStudyRequest): Promise<T.ImportResponse> {
  const resp = await api.createStudy(req);
  setView(resp.study, true);
  wb.main = "board";
  safeStorage("set", "xq-last-study", resp.study.study.id);
  for (const n of resp.notes) notify("warning", n);
  await selectNode(req.kind.type === "record" ? lastMainline(resp.study) : 0);
  return resp;
}

export async function importText(text: string, name: string | null) {
  const resp = await api.importText(text, name);
  setView(resp.study, true);
  wb.main = "board";
  safeStorage("set", "xq-last-study", resp.study.study.id);
  for (const n of resp.notes) notify(resp.stopped ? "warning" : "success", n);
  await selectNode(lastMainline(resp.study));
}

export async function importFromFile() {
  const path = await pickOpenPath();
  if (!path) return;
  const resp = await api.importFile(path);
  setView(resp.study, true);
  wb.main = "board";
  safeStorage("set", "xq-last-study", resp.study.study.id);
  for (const n of resp.notes) notify(resp.stopped ? "warning" : "success", n);
  await selectNode(lastMainline(resp.study));
}

export async function exportStudy() {
  const s = study.value;
  if (!s) return;
  const fileName = `${s.name.replace(/[\\/:*?"<>|]/g, "_")}.json`;
  if (isTauri) {
    const path = await pickSavePath(fileName);
    if (!path) return;
    await api.exportStudyToFile(s.id, path);
    notify("success", `已导出到 ${path}`);
  } else {
    const text = await api.exportStudy(s.id);
    const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = fileName;
    a.click();
    URL.revokeObjectURL(url);
  }
}

export async function updateStudyPrefs(prefs: { name?: string; orientation?: T.Orientation; user_side?: T.Color }) {
  const s = study.value;
  if (!s) return;
  setView(await api.updateStudy({ study_id: s.id, ...prefs }));
}

export async function flipBoard() {
  const s = study.value;
  if (!s) return;
  await updateStudyPrefs({ orientation: s.orientation === "red_bottom" ? "black_bottom" : "red_bottom" });
}
