// 场景 29：写入中途进程终止后的恢复（自带 Pikafish，开发桥带故障注入点）。
// A. 出题写到一半时终止：重启后没有出了一半的题目；再出一次得到完整的一批。
// B. 提交作答时终止：重启后没有记下这次作答、也没有多出学习事件；重新作答只记一次。
// C. AI 讲解生成中强制结束：重启后不会一直显示生成中，写明上次没有完成，可以重新生成；任务登记中记为中断。
// D. 导入学习档案时终止：数据保持导入前的样子。
import {
  BASE,
  LLM,
  bridge,
  check,
  failpoint,
  launch,
  llmCalls,
  llmMode,
  restartBridge,
  sameStored,
  snap,
  stored,
  transportNoise,
  until,
  waitBridgeExit,
} from "../lib.mjs";

const ENDGAME = "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w";
const settled = (id) => until(`会话 ${id} 的讲解完成`, async () => (await bridge("study_view", { id })).stats.pending === 0, 90000);
const idle = () => until("后台任务都结束", async () => (await bridge("job_history")).active.length === 0, 90000);
/** 开发桥在注入点终止（abort） */
async function crashed(what) {
  const exit = await waitBridgeExit();
  check(exit && (exit.signal === "SIGABRT" || (exit.code !== null && exit.code !== 0)), `${what}：开发桥在注入点终止（${JSON.stringify(exit)}）`);
  await restartBridge();
}

const endgame = (
  await bridge("create_study", {
    req: { name: "崩溃残局", kind: { type: "fen", fen: ENDGAME, side: null }, orientation: null, user_side: "red" },
  })
).study.study.id;
const played = await bridge("apply_move", { req: { study_id: endgame, parent: 0, mv: "e2c3", variation: false } });
await settled(endgame);
await bridge("record_practice", {
  req: { study_id: endgame, node_id: played.node_id, hints_used: 0, revealed_directly: false, first_choice: "e2g3", hints: null },
});
await idle();
const threatId = `${endgame}/0000000001/threat`;
const ownCards = async () => (await bridge("list_reviews")).filter((i) => i.card.study_id === endgame);

const { browser, page, problems } = await launch();
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), endgame);
await page.goto(BASE);
await page.waitForSelector(".xq-board");

// ---------- A. 出题写到一半 ----------
await failpoint("exercises:half", "abort");
await page.getByRole("button", { name: "本局报告" }).click();
const report = page.getByTestId("report-dialog");
await report.getByTestId("report-generate").click();
await crashed("出题");
check((await ownCards()).length === 0, "重启后没有出了一半的题目");
await page.reload();
await page.waitForSelector(".xq-board");
await page.getByRole("button", { name: "本局报告" }).click();
await report.getByTestId("report-generate").click();
const generated = await report.getByTestId("report-generated").innerText();
check(generated.includes("找威胁 1"), `再出一次得到完整的一批：${generated}`);
check((await ownCards()).some((i) => i.card.id === threatId), "题目已经加入复习");
await page.keyboard.press("Escape");
await report.waitFor({ state: "hidden" });

// ---------- B. 提交作答时终止 ----------
await idle();
const beforeAttempt = await stored();
await page.getByTestId("open-review").click();
const dialog = page.getByTestId("review-dialog");
const answer = async () => {
  await dialog.locator(`[data-review="${threatId}"]`).click();
  await dialog.locator('[data-testid="review-card"][data-review-state="ready"]').waitFor();
  await dialog.getByTestId("review-card").locator('[data-square="e7"]').first().click();
  await dialog.getByTestId("review-submit-marks").click();
};
await failpoint("attempt:before_commit", "abort");
await answer();
await crashed("作答");
const afterCrash = await stored();
const diff = sameStored(beforeAttempt, afterCrash);
check(diff === "", `重启后没有记下这次作答，也没有多出学习事件${diff ? `：${diff}` : ""}`);
await page.reload();
await page.waitForSelector(".xq-board");
await page.getByTestId("open-review").click();
await answer();
await dialog.getByTestId("review-result").waitFor();
const afterRetry = await stored();
check(afterRetry.cards.find((c) => c.id === threatId).attempts.length === 1, "重新作答只记一次");
check(afterRetry.events.length === beforeAttempt.events.length + 1, "学习事件只多一条");
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });

// ---------- C. AI 讲解生成中强制结束 ----------
const game = (await bridge("import_text", { text: "1. 炮二平五 马8进7", name: "崩溃对局" })).study.study.id;
await settled(game);
await idle();
const status = await bridge("app_status");
await bridge("update_config", {
  config: { ...status.config, coach: { ...status.config.coach, enabled: true, base_url: LLM, model: "mock-model" } },
});
await bridge("set_coach_credential", { req: { key: "sk-e2e-crash-1234" } });
await fetch(`${LLM}/__reset`, { method: "POST" });
await llmMode({ mode: "valid", delay_ms: 60000 });
await page.evaluate((s) => localStorage.setItem("xq-last-study", s), game);
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), game);
await page.reload();
await page.waitForSelector(".xq-board");
await page.locator(".move", { hasText: "炮二平五" }).click();
await page.getByTestId("coach-generate").click();
await page.getByTestId("coach-status").filter({ hasText: "生成中" }).waitFor();
await until("模型服务收到请求", async () => (await llmCalls()).length >= 1);
await restartBridge({ signal: "SIGKILL" });
await llmMode({ mode: "valid", delay_ms: 0 });
await page.reload();
await page.waitForSelector(".xq-board");
await page.locator(".move", { hasText: "炮二平五" }).click();
const coachStatus = page.getByTestId("coach-status");
await coachStatus.filter({ hasText: "没有完成" }).waitFor({ timeout: 15000 });
check(!(await coachStatus.innerText()).includes("生成中"), `重启后不会一直显示生成中：${await coachStatus.innerText()}`);
const jobs = await bridge("job_history");
check(jobs.finished.some((j) => j.kind === "coach" && j.state === "interrupted"), "任务登记中记下上次中断的 AI 讲解");
await page.getByTestId("coach-generate").click();
await coachStatus.filter({ hasText: "已生成" }).waitFor({ timeout: 30000 });
check(true, "重新生成 AI 讲解");
await snap(page, "coach-regenerated");

// ---------- D. 导入学习档案时终止 ----------
await idle();
const archive = await bridge("export_archive");
const beforeImport = await stored();
await failpoint("import:before_commit", "abort");
await bridge("import_archive", { source: { kind: "text", text: archive } }).catch(() => undefined);
await crashed("导入学习档案");
const afterImport = await stored();
const importDiff = sameStored(beforeImport, afterImport);
check(importDiff === "", `导入中途终止后数据保持导入前的样子${importDiff ? `：${importDiff}` : ""}`);

const real = problems.filter((p) => !transportNoise(p));
check(real.length === 0, `控制台无错误${real.length ? `：${real.join("；")}` : ""}`);
await browser.close();
