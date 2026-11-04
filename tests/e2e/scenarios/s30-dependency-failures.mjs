// 场景 30：外部依赖出故障时，界面的等待都会结束，说明原因，可以重试，修复后恢复正常（模拟引擎与模拟模型服务）。
// A. 引擎程序不存在：状态栏写明引擎不可用，讲解以“尚不完整”结束并说明原因，设置的诊断中写明原因；换回引擎后重试得到完整讲解。
// B. 引擎在搜索时退出：讲解以“尚不完整”结束，不会一直显示进行中。
// C. 引擎不回答 readyok：启动中超时后写明不可用。
// D. 模型服务返回 401、429、500，连接中断，返回的不是 JSON，缺少字段：每次都以“生成失败”结束并写明原因，调用次数不超过重试策略，
//    之后服务恢复时重试成功；任务登记中记下每次失败；诊断包中有这些记录且没有密钥。
import fs from "node:fs";
import path from "node:path";
import { BASE, LLM, OUT, bridge, bridgeInfo, check, launch, llmCalls, llmMode, move, readZip, snap, until } from "../lib.mjs";

const SECRET = "sk-e2e-dependency-5678";
const ROUND = `info depth 1 seldepth 1 multipv 1 score cp 30 nodes 10 time 1 pv h2e2 h9g7
info depth 1 seldepth 1 multipv 2 score cp 20 nodes 10 time 1 pv h0g2
bestmove h2e2
`;
const info = await bridgeInfo();
const normal = (await bridge("app_status")).config.engine.binary;

/** 换引擎程序并重新启动引擎 */
async function useEngine(binary) {
  const c = (await bridge("app_status")).config;
  await bridge("update_config", { config: { ...c, engine: { ...c.engine, binary } } });
  await bridge("restart_engine");
}
/** 模拟引擎的另一份副本，按 script 回答 */
function variant(name, script) {
  const dir = path.join(info.data, name);
  fs.mkdirSync(dir, { recursive: true });
  const binary = path.join(dir, path.basename(info.mockEngine));
  fs.copyFileSync(info.mockEngine, binary);
  fs.chmodSync(binary, 0o755);
  fs.writeFileSync(path.join(dir, "mock-uci-script.txt"), script);
  return binary;
}
const engineState = async () => (await bridge("app_status")).engine.state;
async function newStudy(name) {
  const req = { name, kind: { type: "standard" }, orientation: null, user_side: null };
  return (await bridge("create_study", { req })).study.study.id;
}

const { browser, page, problems } = await launch();
let current = await newStudy("引擎不存在");
await page.addInitScript(() => {
  const s = window.localStorage.getItem("e2e-study");
  if (s) window.localStorage.setItem("xq-last-study", s);
});
const open = async (id) => {
  await page.evaluate((s) => localStorage.setItem("e2e-study", s), id);
  await page.reload();
  await page.waitForSelector(".xq-board");
};
await page.goto(BASE);
await open(current);

// ---------- A. 引擎程序不存在 ----------
await useEngine(path.join(info.data, "没有这个引擎", "pikafish"));
await until("引擎不可用", async () => (await engineState()) === "failed");
await page.getByTestId("engine-status").getByText("引擎不可用").waitFor({ timeout: 15000 });
await move(page, "h2e2");
const alert = page.getByTestId("partial-alert");
await alert.waitFor({ timeout: 30000 });
check((await alert.innerText()).includes("讲解尚不完整"), `讲解以“尚不完整”结束并写明原因：${(await alert.innerText()).split("\n")[0]}`);
check((await page.getByTestId("explain-pending").count()) === 0, "不会一直显示进行中");
await page.getByRole("button", { name: "设置" }).click();
const drawer = page.locator(".n-drawer");
await drawer.locator(".n-tabs-tab", { hasText: "诊断" }).click();
check((await drawer.innerText()).includes("引擎：不可用："), "设置的诊断中写明引擎不可用的原因");
await page.keyboard.press("Escape");
await drawer.waitFor({ state: "hidden" });
await useEngine(normal);
await until("引擎恢复", async () => (await engineState()) === "ready");
await alert.getByRole("button", { name: "重试" }).click();
await alert.waitFor({ state: "hidden", timeout: 30000 });
const statusA = Object.values((await bridge("study_view", { id: current })).explain_status);
check(statusA.includes("detailed"), `换回引擎后重试得到完整讲解（${statusA.join("、")}）`);
await snap(page, "engine-restored");

// ---------- B. 引擎在搜索时退出 ----------
await useEngine(variant("engine-exit", "@go\nexit 3\n"));
await until("引擎启动", async () => (await engineState()) === "ready");
// 换一个还没有分析过的局面：同样内容的引擎程序得到的分析结果会直接复用，不再搜索
const kind = { type: "fen", fen: "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w", side: null };
current = (await bridge("create_study", { req: { name: "引擎退出", kind, orientation: null, user_side: null } })).study.study.id;
await open(current);
await move(page, "e2c3");
await alert.waitFor({ timeout: 60000 });
await until("讲解结束", async () => (await bridge("study_view", { id: current })).stats.pending === 0, 60000);
check((await page.getByTestId("explain-pending").count()) === 0, `引擎退出后讲解以“尚不完整”结束：${(await alert.innerText()).split("\n")[0]}`);

// ---------- C. 引擎不回答 readyok ----------
await useEngine(variant("engine-hang", `%ready-hang\n@go\n${ROUND}`));
await until("引擎启动超时", async () => (await engineState()) === "failed", 120000);
await page.getByTestId("engine-status").getByText("引擎不可用").waitFor({ timeout: 15000 });
check(true, "引擎卡在初始化时，超时后写明不可用");
await useEngine(normal);
await until("引擎恢复", async () => (await engineState()) === "ready");

// ---------- D. 模型服务故障 ----------
const status = await bridge("app_status");
await bridge("update_config", {
  config: { ...status.config, coach: { ...status.config.coach, enabled: true, base_url: LLM, model: "mock-model" } },
});
await bridge("set_coach_credential", { req: { key: SECRET } });
await fetch(`${LLM}/__reset`, { method: "POST" });
current = await newStudy("模型服务故障");
await bridge("apply_move", { req: { study_id: current, parent: 0, mv: "h2e2", variation: false } });
await until("讲解完成", async () => Object.values((await bridge("study_view", { id: current })).explain_status).includes("detailed"));
await open(current);
await page.locator(".move", { hasText: "炮二平五" }).click();
const card = page.getByTestId("coach-card");
const coachStatus = page.getByTestId("coach-status");
const maxCalls = (await bridge("app_status")).coach.calls_per_move.max;
const reasons = [];
for (const mode of ["401", "429", "500", "drop", "invalid-json", "missing-fields"]) {
  await llmMode({ mode, delay_ms: 0, script: [] });
  const before = (await llmCalls()).length;
  const ref = { req: { study_id: current, node_id: 1 } };
  const previous = (await bridge("coach_view", ref)).record?.updated_at ?? 0;
  const shown = (await coachStatus.count()) ? await coachStatus.innerText() : "";
  const generate = card.getByRole("button", { name: /生成 AI 讲解|重试/ });
  await generate.first().click();
  // 等这一次请求的结果：后台记下了新的失败，界面上的原因也换成了这一次的
  await until(`${mode} 的结果`, async () => {
    const v = await bridge("coach_view", ref);
    return v.status === "failed" && (v.record?.updated_at ?? 0) > previous;
  }, 60000);
  await until(`${mode} 的结果显示在界面上`, async () => {
    const t = await coachStatus.innerText();
    return t.includes("生成失败") && t !== shown;
  });
  const text = await coachStatus.innerText();
  const calls = (await llmCalls()).length - before;
  reasons.push(`${mode}：${text.replace("AI 讲解生成失败：", "")}`);
  check(calls >= 1 && calls <= maxCalls, `${mode}：以“生成失败”结束并写明原因，调用 ${calls} 次（不超过 ${maxCalls} 次）`);
  check(!text.includes(SECRET), `${mode}：原因中没有密钥`);
}
console.log(`  失败原因：${reasons.join("；")}`);
await llmMode({ mode: "valid", delay_ms: 0, script: [] });
await card.getByRole("button", { name: "重试" }).click();
await coachStatus.filter({ hasText: "已生成" }).waitFor({ timeout: 30000 });
check(true, "模型服务恢复后重试成功");
const jobs = await bridge("job_history");
const failedCoach = jobs.finished.filter((j) => j.kind === "coach" && j.state === "failed");
check(failedCoach.length >= 6 && failedCoach.every((j) => j.operation_id && j.detail), `任务登记中记下每次失败与原因（${failedCoach.length} 次）`);

// 诊断包：通过界面导出，包含任务登记，没有密钥
await page.getByRole("button", { name: "设置" }).click();
await drawer.locator(".n-tabs-tab", { hasText: "诊断" }).click();
const [download] = await Promise.all([page.waitForEvent("download"), drawer.getByTestId("diag-bundle").click()]);
const file = path.join(OUT, "s30-diagnostics.zip");
await download.saveAs(file);
const bundle = readZip(fs.readFileSync(file));
check(["manifest.json", "job-history.json", "configuration-redacted.json"].every((n) => n in bundle), `诊断包含有清单、任务登记与设置：${Object.keys(bundle).join("、")}`);
const everything = Object.values(bundle).join("\n");
check(!everything.includes(SECRET), "诊断包中没有密钥");
check(!everything.includes(info.data), "诊断包中没有本机的数据目录");
check(JSON.parse(bundle["job-history.json"]).finished.some((j) => j.kind === "coach" && j.state === "failed"), "诊断包的任务登记中有 AI 讲解的失败记录");
await page.keyboard.press("Escape");

check(problems.length === 0, `控制台无错误${problems.length ? `：${problems.join("；")}` : ""}`);
await browser.close();
