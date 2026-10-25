// 场景 14：自动 AI 讲解的触发节奏与调用预算。核对模拟模型服务实际收到的请求与结果归属。
// A. 在一着上停留后才自动生成；快速翻阅经过的着法不发请求；
// B. 回到已有结果的着法、切换布局、演示，都不再调用模型；
// C. 离开自动请求过的着法：尚未发出的请求不再发送，已经发出的照常完成并归入原来的着法；
// D. 批量生成：生成前说明要生成几着、调用几次、今天还剩几次；失败可以重试；暂停与继续；停止；次数用完自动停止。
import { BASE, bridge, check, launch, snap } from "../lib.mjs";

const LLM = process.env.MOCK_LLM ?? "http://127.0.0.1:1440";
const RECORD = "1. 炮二平五 马8进7 2. 马二进三 车9平8 3. 车一平二 马2进3 4. 兵七进一 卒7进1";
const NAMES = ["", "炮二平五", "马8进7", "马二进三", "车9平8", "车一平二", "马2进3", "兵七进一", "卒7进1"];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const mock = (body) => fetch(`${LLM}/__mode`, { method: "POST", body: JSON.stringify(body) });
/** 模拟模型服务收到的请求，按这一着的记法 */
const calls = async () => (await (await fetch(`${LLM}/__calls`)).json()).map((c) => c.notation);
const callsSince = async (n) => (await calls()).slice(n);

const created = await bridge("create_study", { req: { name: "AI 讲解节奏", kind: { type: "record", text: RECORD }, orientation: null, user_side: null } });
const sid = created.study.study.id;
for (let i = 0; i < 240; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 8) break;
  await sleep(500);
}
const coachOf = (id) => bridge("coach_view", { req: { study_id: sid, node_id: id } });
async function waitCoach(id, pred, what, ms = 60000) {
  const deadline = Date.now() + ms;
  for (;;) {
    const v = await coachOf(id);
    if (pred(v)) return v;
    if (Date.now() > deadline) throw new Error(`等待超时：${what}（状态 ${v.status}）`);
    await sleep(200);
  }
}
const isReady = (v) => v.status === "ready" && v.result_current;

// 查看时自动生成；一次只发一个请求，便于观察排队中的请求
await fetch(`${LLM}/__reset`, { method: "POST" });
const original = (await bridge("app_status")).config.coach;
// 每次都在当前配置上修改：配置中的密钥引用在保存密钥后才有
async function coachConfig(extra = {}) {
  const base = (await bridge("app_status")).config;
  const coach = {
    ...base.coach,
    enabled: true,
    provider: "deepseek",
    base_url: LLM,
    model: "mock-model",
    auto_explain_scope: "viewed",
    max_parallel_requests: 1,
    daily_request_budget: 0,
    audience: "beginner",
    detail: "normal",
  };
  return { ...base, coach: { ...coach, ...extra } };
}
await bridge("update_config", { config: await coachConfig() });
await bridge("set_coach_credential", { req: { key: "sk-e2e-pacing" } });
// 收藏两着，供批量生成使用（在界面中收藏需要先选中，会触发自动生成）
for (const id of [5, 7]) await bridge("set_note", { req: { study_id: sid, node_id: id, note: "", starred: true } });

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");

// ---------- A. 停留后才生成 ----------
// 打开会话时停在最后一着上
await waitCoach(8, isReady, "打开时停留的着法生成 AI 讲解");
check((await calls()).join() === NAMES[8], `停留在打开的着法上，只为它调用一次模型：${(await calls()).join("、")}`);

// 快速向前翻阅六着，停在第 2 着
let before = (await calls()).length;
for (let i = 0; i < 6; i++) {
  await page.keyboard.press("ArrowLeft");
  await sleep(120);
}
await page.locator('[data-node="2"] .notation').waitFor();
await waitCoach(2, isReady, "停下来的着法生成 AI 讲解");
await sleep(1500);
let added = await callsSince(before);
check(added.join() === NAMES[2], `快速经过的五着不发请求，只为停下来的着法调用一次：${added.join("、")}`);
const passed = await Promise.all([3, 4, 5, 6, 7].map(coachOf));
check(passed.every((v) => v.status === "none"), "经过的着法没有排队或生成");
check((await coachOf(2)).record.result.output.summary.text.startsWith(NAMES[2]), "结果归入停下来的那一着");

// ---------- B. 有效结果直接使用 ----------
before = (await calls()).length;
for (let i = 0; i < 6; i++) {
  await page.keyboard.press("ArrowRight");
  await sleep(120);
}
await sleep(1500);
check((await page.getByTestId("coach-status").innerText()).includes("已生成"), "回到已有结果的着法直接显示结果");
await page.setViewportSize({ width: 800, height: 900 });
await sleep(800);
await page.setViewportSize({ width: 1440, height: 900 });
await sleep(800);
await page.getByRole("button", { name: "演示主要应对" }).click();
await page.getByTestId("board-context").filter({ hasText: "正在演示" }).waitFor();
await page.keyboard.press("ArrowRight");
await sleep(1000);
await page.keyboard.press("Escape");
await sleep(1000);
added = await callsSince(before);
check(added.length === 0, `重新经过、回到已有结果的着法、切换布局、演示，都不调用模型：${added.join("、") || "没有请求"}`);

// ---------- C. 离开时只取消尚未发出的请求 ----------
await mock({ delay_ms: 4000 });
before = (await calls()).length;
await page.locator('[data-node="3"]').click();
// 停留后发出第 3 着的请求（模型服务 4 秒后才回复）
for (let i = 0; i < 40 && !(await callsSince(before)).length; i++) await sleep(100);
check((await callsSince(before)).join() === NAMES[3], "停留后发出第 3 着的请求");
await page.locator('[data-node="4"]').click();
await waitCoach(4, (v) => v.status === "queued", "第 4 着的请求排在第 3 着之后", 5000);
// 离开第 4 着（去已有结果的第 2 着）：它还没有发出，不再发送
await page.locator('[data-node="2"]').click();
await waitCoach(4, (v) => v.status === "none", "离开后第 4 着的排队请求被取消", 5000);
const third = await waitCoach(3, isReady, "已经发出的第 3 着照常完成", 20000);
await sleep(1500);
added = await callsSince(before);
check(added.join() === NAMES[3], `离开第 3 着后它的请求照常完成，第 4 着的请求没有发出：${added.join("、")}`);
check(third.record.result.output.summary.text.startsWith(NAMES[3]), "已经发出的请求的结果归入第 3 着");
check((await coachOf(4)).record?.result == null, "第 4 着没有结果");
await mock({ delay_ms: 300 });

// ---------- D. 批量生成 ----------
const dialog = page.getByTestId("coach-batch");
const summary = () => dialog.getByTestId("batch-summary").innerText();
const phase = () => dialog.getByTestId("batch-phase").innerText();
const waitPhase = (text, ms = 60000) => dialog.getByTestId("batch-phase").filter({ hasText: text }).waitFor({ timeout: ms });
const itemStates = () => dialog.locator("[data-batch-node]").evaluateAll((els) => els.map((e) => `${e.dataset.batchNode}:${e.dataset.state}`));
async function openBatch(scope) {
  if (!(await dialog.isVisible())) await page.getByTestId("coach-batch-open").click();
  await dialog.waitFor();
  await dialog.getByTestId(`batch-scope-${scope}`).click();
  await dialog.getByTestId("batch-summary").waitFor();
  await sleep(300);
}
async function closeBatch() {
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
}

// 生成前的说明：主线 8 着中已有 3 着（第 2、3、8 着），需要生成 5 着
await openBatch("mainline");
let text = await summary();
check(text.includes("共 8 着") && text.includes("已有讲解 3 着") && text.includes("需要生成 5 着"), `生成前说明要生成几着：${text.replace(/\n/g, " ")}`);
check(text.includes("预计调用模型 5–10 次") && text.includes("不限次数"), "说明预计调用几次与今天的剩余次数");
check((await dialog.getByTestId("batch-scope-starred").innerText()).includes("2 着"), "收藏的着法有 2 着");
check((await calls()).length === before + 1, "只打开批量生成不调用模型");
await snap(page, "batch-plan");

// 失败与重试：模型输出不合格时，每着调用两次（含一次修正）后记为失败
await mock({ mode: "bad" });
await openBatch("starred");
check((await summary()).includes("需要生成 2 着"), "收藏的着法都需要生成");
before = (await calls()).length;
await dialog.getByTestId("batch-start").click();
await waitPhase("2 着失败");
added = await callsSince(before);
check(added.join() === [NAMES[5], NAMES[5], NAMES[7], NAMES[7]].join(), `逐着生成收藏的着法，输出不合格时多一次修正：${added.join("、")}`);
check((await itemStates()).join() === "5:failed,7:failed", "两着都记为失败");
await snap(page, "batch-failed");
await mock({ mode: "valid" });
before = (await calls()).length;
await dialog.getByTestId("batch-retry").click();
await waitPhase("全部完成");
added = await callsSince(before);
check(added.join() === [NAMES[5], NAMES[7]].join(), `重试失败的着法：${added.join("、")}`);
check((await itemStates()).join() === "5:done,7:done", "重试后都已生成");
check((await coachOf(5)).record.result.output.summary.text.startsWith(NAMES[5]), "批量生成的结果归入各自的着法");

// 暂停与继续：当前这一着完成后暂停，暂停期间不再发请求
await mock({ delay_ms: 1500 });
await openBatch("mainline");
check((await summary()).includes("需要生成 3 着"), `整条主线还需要生成 3 着：${(await summary()).replace(/\n/g, " ")}`);
before = (await calls()).length;
await dialog.getByTestId("batch-start").click();
await dialog.locator('[data-state="running"]').waitFor();
await dialog.getByTestId("batch-pause").click();
check((await phase()).includes("当前这一着完成后暂停"), "暂停时说明当前这一着完成后暂停");
await waitPhase("已暂停");
const paused = (await calls()).length;
await sleep(2500);
check((await calls()).length === paused && paused === before + 1, `暂停期间不再发请求（暂停前发出 ${paused - before} 个）`);
check((await itemStates()).join() === "1:done,4:waiting,6:waiting", `暂停时第一着已完成：${(await itemStates()).join()}`);
await snap(page, "batch-paused");
await dialog.getByTestId("batch-resume").click();
await waitPhase("全部完成");
added = await callsSince(before);
check(added.join() === [NAMES[1], NAMES[4], NAMES[6]].join(), `继续后生成其余的着法，每着一次：${added.join("、")}`);
await closeBatch();

// 停止：改变学习水平使已有结果过期，开始后立即停止
await bridge("update_config", { config: await coachConfig({ audience: "intermediate" }) });
await mock({ delay_ms: 2500 });
await openBatch("mainline");
check((await summary()).includes("需要生成 8 着"), "改变学习水平后，主线的结果都需要重新生成");
before = (await calls()).length;
await dialog.getByTestId("batch-start").click();
await dialog.locator('[data-state="running"]').waitFor();
for (let i = 0; i < 40 && !(await callsSince(before)).length; i++) await sleep(100);
await dialog.getByTestId("batch-stop").click();
await waitPhase("已停止");
await waitCoach(1, isReady, "停止时已经发出的请求照常完成", 20000);
await sleep(1500);
added = await callsSince(before);
check(added.join() === NAMES[1], `停止后不再发请求，已经发出的请求完成并归入原来的着法：${added.join("、")}`);
await closeBatch();

// 次数用完自动停止：今天只剩 1 次
const used = (await bridge("app_status")).coach.used_today;
await bridge("update_config", { config: await coachConfig({ audience: "intermediate", daily_request_budget: used + 1 }) });
await mock({ delay_ms: 300 });
await openBatch("mainline");
text = await summary();
check(text.includes("需要生成 7 着") && text.includes("还剩 1 次"), `说明今天还剩几次：${text.replace(/\n/g, " ")}`);
check(await dialog.getByText("今天剩余的次数可能不够生成全部").isVisible(), "剩余次数不够时提示");
before = (await calls()).length;
await dialog.getByTestId("batch-start").click();
await waitPhase("今天的调用次数已用完");
await sleep(1000);
added = await callsSince(before);
check(added.join() === NAMES[2], `次数用完后自动停止：${added.join("、")}`);
check((await itemStates()).filter((s) => s.endsWith(":waiting")).length === 6, "其余的着法没有生成");
check(await dialog.getByTestId("batch-start").isDisabled(), "次数用完后不能继续生成，并说明可以调高上限");
await snap(page, "batch-budget");
await closeBatch();

// 恢复设置，不影响之后的场景
await bridge("update_config", { config: { ...(await bridge("app_status")).config, coach: original } });
await fetch(`${LLM}/__reset`, { method: "POST" });

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
