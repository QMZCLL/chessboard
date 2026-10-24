// 后台调用与事件订阅。在 Tauri 中使用 IPC；在浏览器中调试时转发给开发桥（pnpm bridge）。

import type * as T from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const BRIDGE = "/__bridge";

export class BackendError extends Error {}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

async function call<R>(cmd: string, args: Record<string, unknown> = {}): Promise<R> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      return await invoke<R>(cmd, args);
    } catch (e) {
      throw new BackendError(errorMessage(e));
    }
  }
  let resp: Response;
  try {
    resp = await fetch(`${BRIDGE}/invoke/${cmd}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(args),
    });
  } catch (e) {
    throw new BackendError(`无法连接开发桥：${errorMessage(e)}`);
  }
  const data = await resp.json().catch(() => null);
  if (!resp.ok) throw new BackendError(data?.message ?? `请求失败（HTTP ${resp.status}）`);
  return data as R;
}

const EVENTS = [
  "engine_status",
  "analysis_progress",
  "analysis_done",
  "explanation_updated",
  "study_updated",
  "capture_state",
  "cloud_result",
  "coach_updated",
  "comparison_updated",
] as const;
export type EventName = (typeof EVENTS)[number];

type Handler = (payload: unknown) => void;
const handlers = new Map<string, Set<Handler>>();
let subscribed: Promise<void> | null = null;

function dispatch(event: string, payload: unknown) {
  handlers.get(event)?.forEach((h) => {
    try {
      h(payload);
    } catch (e) {
      console.error(`处理事件 ${event} 出错`, e);
    }
  });
}

function subscribe(): Promise<void> {
  if (subscribed) return subscribed;
  subscribed = (async () => {
    if (isTauri) {
      const { listen } = await import("@tauri-apps/api/event");
      for (const name of EVENTS) await listen(name, (e) => dispatch(name, e.payload));
    } else {
      const source = new EventSource(`${BRIDGE}/events`);
      source.onmessage = (m) => {
        const { event, payload } = JSON.parse(m.data);
        dispatch(event, payload);
      };
    }
  })();
  return subscribed;
}

/** 订阅后台事件，返回取消订阅的函数。 */
export function on<P>(event: EventName, handler: (payload: P) => void): () => void {
  void subscribe();
  const set = handlers.get(event) ?? new Set<Handler>();
  set.add(handler as Handler);
  handlers.set(event, set);
  return () => set.delete(handler as Handler);
}

export function ready(): Promise<void> {
  return subscribe();
}

export const api = {
  startupError: () => call<string | null>("startup_error"),
  status: () => call<T.AppStatus>("app_status"),
  updateConfig: (config: T.AppConfig) => call<T.AppStatus>("update_config", { config }),
  restartEngine: () => call<void>("restart_engine"),
  listStudies: () => call<T.StudySummary[]>("list_studies"),
  createStudy: (req: T.CreateStudyRequest) => call<T.ImportResponse>("create_study", { req }),
  validatePosition: (fen: string, side: T.Color | null) =>
    call<T.ValidateResponse>("validate_position", { req: { fen, side } }),
  loadStudy: (id: string) => call<T.StudyView>("load_study", { id }),
  studyView: (id: string) => call<T.StudyView>("study_view", { id }),
  updateStudy: (req: { study_id: string; name?: string; orientation?: T.Orientation; user_side?: T.Color }) =>
    call<T.StudyView>("update_study", { req }),
  deleteStudy: (id: string) => call<void>("delete_study", { id }),
  nodeDetail: (study_id: string, node_id: number) => call<T.NodeDetail>("node_detail", { req: { study_id, node_id } }),
  applyMove: (study_id: string, parent: number, mv: T.Move, variation: boolean) =>
    call<T.MoveResponse>("apply_move", { req: { study_id, parent, mv, variation } }),
  addLine: (study_id: string, parent: number, moves: T.Move[]) =>
    call<T.MoveResponse>("add_line", { req: { study_id, parent, moves } }),
  deleteNode: (study_id: string, node_id: number) => call<T.StudyView>("delete_node", { req: { study_id, node_id } }),
  promoteNode: (study_id: string, node_id: number) => call<T.StudyView>("promote_node", { req: { study_id, node_id } }),
  /** 记录一次先想后讲练习：用了几级提示、是否直接看讲解、自己走出的第一步 */
  recordPractice: (study_id: string, node_id: number, hints_used: number, revealed_directly: boolean, first_choice: T.Move | null) =>
    call<T.StudyView>("record_practice", { req: { study_id, node_id, hints_used, revealed_directly, first_choice } }),
  setNote: (study_id: string, node_id: number, note: string, starred: boolean) =>
    call<T.StudyView>("set_note", { req: { study_id, node_id, note, starred } }),
  focusNode: (study_id: string, node_id: number) => call<void>("focus_node", { req: { study_id, node_id } }),
  /** retry 为 true 时，部分完成的讲解重新分析，不复用不完整的结果。 */
  explainMove: (study_id: string, node_id: number, budget: T.Budget, retry = false) =>
    call<T.ExplanationRecord | null>("explain_move", { req: { study_id, node_id, budget, retry } }),
  coachExplain: (study_id: string, node_id: number, refresh = false) =>
    call<T.CoachView>("coach_explain", { req: { study_id, node_id, refresh } }),
  coachCancel: (study_id: string, node_id: number) => call<T.CoachView>("coach_cancel", { req: { study_id, node_id } }),
  coachView: (study_id: string, node_id: number) => call<T.CoachView>("coach_view", { req: { study_id, node_id } }),
  /** 保存或清除模型服务的密钥（只写入后台凭据文件，不会再返回界面）。 */
  setCoachCredential: (key: string | null) => call<T.AppStatus>("set_coach_credential", { req: { key } }),
  /** 对比同一局面下的两种走法；reference 为空时由后台选择对照（实际着法、引擎首选或其他分支） */
  compareMoves: (study_id: string, node_id: number, mine: T.Move, reference: T.Move | null) =>
    call<T.CompareView>("compare_moves", { req: { study_id, node_id, mine, reference } }),
  analyzePosition: (study_id: string, node_id: number, budget: T.Budget, request_id: string) =>
    call<void>("analyze_position", { req: { study_id, node_id, budget, request_id } }),
  cancelAnalysis: (requestId: string) => call<void>("cancel_analysis", { requestId }),
  cloudLookup: (study_id: string, node_id: number) => call<void>("cloud_lookup", { req: { study_id, node_id } }),
  exportStudy: (id: string) => call<string>("export_study", { id }),
  exportStudyToFile: (id: string, path: string) => call<void>("export_study_to_file", { id, path }),
  importFile: (path: string) => call<T.ImportResponse>("import_file", { path }),
  importText: (text: string, name: string | null) => call<T.ImportResponse>("import_text", { text, name }),
  diagnostics: () => call<string>("diagnostics"),
  openLogDir: () => call<void>("open_log_dir"),
  listWindows: () => call<T.WindowInfo[]>("list_windows"),
  captureSnapshot: (windowId: number) => call<T.Snapshot>("capture_snapshot", { windowId }),
  startCapture: (window_id: number, rect: T.Rect) => call<T.CaptureState>("start_capture", { req: { window_id, rect } }),
  stopCapture: () => call<T.CaptureState>("stop_capture"),
  confirmCaptureStart: (req: {
    board: string;
    side: T.Color;
    orientation: T.Orientation;
    name: string | null;
    attach: { study_id: string; node_id: number } | null;
  }) => call<T.StudyView>("confirm_capture_start", { req }),
  resolveCapture: (
    req: { action: "accept"; board: string; side: T.Color } | { action: "new_study"; board: string; side: T.Color } | { action: "dismiss" },
  ) => call<T.CaptureState>("resolve_capture", { req }),
  relocateCapture: () => call<T.CaptureState>("relocate_capture"),
  /** 仅开发桥：设置模拟画面 */
  devShowBoard: (board: string | null, orientation: T.Orientation) => call<void>("dev_show_board", { board, orientation }),
};

/** 选择保存位置（仅桌面应用）。 */
export async function pickSavePath(defaultName: string): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  return await save({ defaultPath: defaultName, filters: [{ name: "象棋学习数据", extensions: ["json"] }] });
}

/** 选择要导入的文件（仅桌面应用）。 */
export async function pickOpenPath(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "棋谱、FEN 或学习数据", extensions: ["json", "pgn", "txt", "fen", "xqf"] }],
  });
  return typeof result === "string" ? result : null;
}

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    document.body.appendChild(area);
    area.select();
    document.execCommand("copy");
    area.remove();
  }
}

export function newRequestId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}
