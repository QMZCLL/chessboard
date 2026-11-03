// 学习工作台：启动与后台事件的订阅。状态与操作分在同目录的几个文件中，这里统一导出，组件都从这里取用。

import { api, errorMessage, on, ready } from "../api/backend";
import type * as T from "../api/types";
import { onCoach } from "./coach";
import { notify, safeStorage, wb } from "./core";
import { openStudy, onStudyUpdated } from "./session";
import { onExplanation } from "./studyView";

export * from "./core";
export * from "./studyView";
export * from "./coach";
export * from "./think";
export * from "./display";
export * from "./navigation";
export * from "./preview";
export * from "./session";

// ---------- 启动与事件 ----------

export async function boot() {
  await ready();
  on<T.EngineState>("engine_status", (s) => {
    if (wb.status) wb.status.engine = s;
  });
  on<T.ExplanationEvent>("explanation_updated", onExplanation);
  on<T.StudyUpdatedEvent>("study_updated", onStudyUpdated);
  on<T.CoachView>("coach_updated", onCoach);
  on<T.AnalysisProgressEvent>("analysis_progress", (p) => {
    const a = wb.analysis;
    if (!a || a.requestId !== p.request_id) return;
    a.depth = p.depth;
    a.candidates = p.candidates;
  });
  on<T.AnalysisDoneEvent>("analysis_done", (p) => {
    const a = wb.analysis;
    if (!a || a.requestId !== p.request_id) return;
    a.running = false;
    a.position = p.position;
    a.candidates = p.position.candidates;
    a.error = p.error;
    if (p.cancelled) a.error = "分析已取消";
  });
  on<T.CaptureState>("capture_state", (s) => {
    wb.capture = s;
    if (s.state === "awaiting_start" || s.state === "awaiting_confirmation") wb.dialogs.capture = true;
    if (s.state === "stopped") notify("warning", `屏幕连接已停止：${s.message}`);
  });
  on<T.CloudEvent>("cloud_result", (p) => {
    if (p.error) notify("warning", p.error);
    if (p.record && wb.detail && wb.detail.study_id === p.study_id && wb.detail.node.id === p.node_id) {
      wb.detail.cloud = p.record;
    }
  });
  try {
    wb.startupError = await api.startupError();
    if (wb.startupError) {
      wb.booted = true;
      return;
    }
    wb.status = await api.status();
    wb.capture = wb.status.capture;
    for (const n of wb.status.notices) notify("warning", n);
    const last = safeStorage("get", "xq-last-study");
    if (last) {
      try {
        await openStudy(last);
      } catch {
        wb.dialogs.newStudy = true;
      }
    } else {
      wb.dialogs.newStudy = true;
    }
  } catch (e) {
    wb.startupError = errorMessage(e);
  }
  wb.booted = true;
}
