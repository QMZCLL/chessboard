// 场景 16：教练追问。核对模拟模型服务实际收到的问题与回答的归属。
// A. AI 讲解未启用时，追问区说明原因、可以去设置，问题不能提出；
// B. 对比搜索完成后提问：回答下方是程序结论原文作为依据，演示程序选择的变化，“想一想”留给学习者验证；
//    请求中只有这组对比的证据；再次打开这组对比时直接显示回答，不再调用模型；
// C. 输出不合格时修正一次后显示，修正后仍不合格时说明原因、可以重新提问；回答期间关闭对比、换到别的着法，
//    回答仍归属原来的问题；每次调用都计入今天的次数；
// D. 对比面板保持打开时在设置中修改学习水平：回答立即标为过期；设置改变前读取、晚到的旧状态不能把它恢复为当前有效；
//    可以重新回答。
import { BASE, bridge, check, hold, launch, move, snap } from "../lib.mjs";

const LLM = process.env.MOCK_LLM ?? "http://127.0.0.1:1440";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const mock = (body) => fetch(`${LLM}/__mode`, { method: "POST", body: JSON.stringify(body) });
const calls = async () => (await (await fetch(`${LLM}/__calls`)).json()).filter((c) => c.question);

const created = await bridge("create_study", {
  req: { name: "教练追问", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 2) break;
  await sleep(500);
}

// 先关闭 AI 讲解；结束时恢复原来的设置
const original = (await bridge("app_status")).config.coach;
async function coachConfig(extra) {
  const base = (await bridge("app_status")).config;
  const coach = { ...base.coach, provider: "deepseek", base_url: LLM, model: "mock-model", auto_explain_scope: "off", daily_request_budget: 0, audience: "beginner" };
  await bridge("update_config", { config: { ...base, coach: { ...coach, ...extra } } });
}
await coachConfig({ enabled: false });
await fetch(`${LLM}/__reset`, { method: "POST" });

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => {
  localStorage.setItem("xq-last-study", id);
  localStorage.setItem("xq-detail-open", "1");
}, sid);
const panel = page.getByTestId("compare-panel");
const askArea = page.getByTestId("coach-ask");
const ctx = page.getByTestId("board-context");
const answerOf = (kind) => page.getByTestId(`coach-answer-${kind}`);

/** 从第 1 着之后试走马2进3，与实际着法马8进7对比，等同一次搜索完成 */
async function openTrialCompare() {
  await page.locator('[data-node="1"]').click();
  await page.getByRole("button", { name: "从这里试走" }).click();
  await move(page, "b9c7");
  await page.getByTestId("compare-trial").click();
  await panel.waitFor();
  await page.getByTestId("compare-outcome").waitFor({ timeout: 60000 });
  await panel.getByText("正在用同一次搜索比较两步").waitFor({ state: "detached", timeout: 60000 });
}
async function closeCompare() {
  await page.getByTestId("compare-close").click();
  await panel.waitFor({ state: "detached" });
  await page.getByRole("button", { name: "结束试走，返回原局面" }).click();
}

// ---------- A. AI 讲解未启用 ----------
await page.goto(BASE);
await page.waitForSelector(".xq-board");
await openTrialCompare();
const unavailable = await askArea.getByTestId("coach-ask-unavailable").innerText();
check(unavailable.includes("未启用") && unavailable.includes("去设置"), `说明不能提问的原因：${unavailable}`);
check(await askArea.getByTestId("coach-ask-my_move").isDisabled(), "AI 讲解未启用时问题不能提出");
await closeCompare();

// ---------- B. 提问与回答 ----------
await coachConfig({ enabled: true });
await bridge("set_coach_credential", { req: { key: "sk-e2e-ask" } });
await page.reload();
await page.waitForSelector(".xq-board");
await openTrialCompare();
const labels = await askArea.locator("[data-testid^=coach-ask-] .n-button__content").allInnerTexts();
check(
  ["我的选择马2进3起了什么作用？", "马2进3与马8进7的差别在哪里？", "走马2进3之后，对方的回应需要怎样应对？"].every((q) => labels.some((l) => l.includes(q))),
  `三个结构化问题：${labels.join(" / ")}`,
);
check((await askArea.getByTestId("coach-ask-budget").innerText()).includes("每个问题通常调用模型 1 次"), "说明每个问题调用模型的次数");
const usedBefore = (await bridge("app_status")).coach.used_today;
await askArea.getByTestId("coach-ask-my_move").click();
const mine = answerOf("my_move");
await mine.getByTestId("coach-answer-text").waitFor({ timeout: 30000 });
const mineText = await mine.innerText();
check(mineText.includes("我的选择马2进3起了什么作用？") && mineText.includes("马2进3改变了这一侧的子力关系"), "显示问题与回答");
const basis = await mine.getByTestId("coach-basis").first().innerText();
check(basis.includes("依据") && basis.includes("马2进3："), `回答下方是程序结论原文：${basis.replace(/\n/g, " ").slice(0, 80)}`);
check((await mine.getByTestId("coach-answer-check").innerText()).includes("想一想"), "“想一想”留给学习者验证");
check((await askArea.getByTestId("coach-ask-my_move").count()) === 0, "已经回答的问题不再列为可以提出的问题");
await mine.getByTestId("coach-answer-demo-my_move").first().click();
const c = await ctx.innerText();
check(c.includes("演示我的选择 马2进3（从比较起点）"), `演示程序选择的变化，从比较起点开始：${c}`);
await page.keyboard.press("Escape");
let sent = await calls();
const ev = JSON.parse(sent[0].body.messages[1].content).evidence;
check(
  sent.length === 1 && sent[0].question === "my_move" && ev.mine.notation === "马2进3" && ev.reference.notation === "马8进7" && ev.question.text === "我的选择马2进3起了什么作用？",
  "请求中是这组对比与这个问题",
);
check(!JSON.stringify(sent[0].body).includes("sk-e2e-ask") && sent[0].authorization === "Bearer sk-e2e-ask", "密钥只在请求头中");
check(ev.claims.every((cl) => ["mine", "reference", "mine_reply", "reference_reply", "difference", "score"].includes(cl.group)), "证据只来自这组对比");
await snap(page, "coach-answer");

// ---------- C. 修正、离开后的归属、调用次数 ----------
// 对方回应：第一次输出不合格，修正后显示
await mock({ script: ["bad", "valid"] });
await askArea.getByTestId("coach-ask-reply").click();
await answerOf("reply").getByTestId("coach-answer-text").waitFor({ timeout: 30000 });
sent = await calls();
check(sent.length === 3 && sent.slice(1).every((s) => s.question === "reply"), `不合格的输出修正一次后显示（共 ${sent.length} 次请求）`);
check((await answerOf("reply").getByTestId("coach-answer-demo-reply").innerText()).includes("演示对方的回应"), "对方回应的问题演示我的选择之后的变化");
// 两步的差别：回答期间关闭对比、换到别的着法
await mock({ delay_ms: 1500 });
await askArea.getByTestId("coach-ask-difference").click();
await answerOf("difference").getByText("等待回答").or(answerOf("difference").getByText("正在回答")).first().waitFor();
await closeCompare();
await page.locator('[data-node="2"]').click();
const pair = { study_id: sid, node_id: 1, mine: "b9c7", reference: "h9g7" };
for (let i = 0; i < 100; i++) {
  const list = await bridge("coach_answers", { req: pair });
  if (list.some((a) => a.question.kind === "difference" && a.status === "ready")) break;
  await sleep(200);
}
await mock({ delay_ms: 0 });
check((await page.getByTestId("compare-panel").count()) === 0, "离开对比后，回答不会把对比重新打开");
await openTrialCompare();
check((await answerOf("difference").getByTestId("coach-answer-text").count()) === 1, "回到这组对比时，离开期间完成的回答归属原来的问题");
check(
  (await answerOf("my_move").getByTestId("coach-answer-text").count()) === 1 && (await calls()).length === 4,
  "再次打开这组对比时直接显示已有的回答，不再调用模型",
);
const usedAfter = (await bridge("app_status")).coach.used_today;
check(usedAfter - usedBefore === 4, `每次调用都计入今天的次数（增加 ${usedAfter - usedBefore} 次）`);
// 换一个对照：没有这组的回答
await page.getByTestId("compare-reference-select").click();
await page.locator(".n-base-select-option", { hasText: "引擎候选第" }).first().click();
await page.getByTestId("compare-outcome").waitFor({ timeout: 60000 });
await panel.getByText("正在用同一次搜索比较两步").waitFor({ state: "detached", timeout: 60000 });
check((await page.locator("[data-testid^=coach-answer-]").count()) === 0, "换一个对照后，不显示另一组对比的回答");
// 修正后仍不合格：说明原因，可以重新提问
await mock({ script: ["bad", "bad"] });
await askArea.getByTestId("coach-ask-my_move").click();
const failed = answerOf("my_move").getByTestId("coach-answer-error");
await failed.waitFor({ timeout: 30000 });
check((await failed.innerText()).includes("没有通过检查") && (await answerOf("my_move").getByTestId("coach-answer-text").count()) === 0, `回答没有通过检查时不显示，并说明原因：${(await failed.innerText()).slice(0, 60)}`);
await failed.getByRole("button", { name: "重新提问" }).click();
await answerOf("my_move").getByTestId("coach-answer-text").waitFor({ timeout: 30000 });
check((await answerOf("my_move").getByTestId("coach-answer-error").count()) === 0, "重新提问后显示回答");
await closeCompare();

// ---------- D. 设置改变后过期 ----------
// 打开这组对比时读取回答状态，拦住这次的响应（设置改变前读取的旧状态）
let firstRead = true;
const oldRead = await hold(page, "coach_answers", {
  match: () => {
    const m = firstRead;
    firstRead = false;
    return m;
  },
});
await openTrialCompare();
await oldRead.arrived;
// 对比面板保持打开，在设置中把学习水平改为高级
await page.getByRole("button", { name: "设置" }).click();
const drawer = page.locator(".n-drawer");
await drawer.locator(".n-form-item", { hasText: "学习水平" }).locator(".n-base-selection").click();
await page.locator(".n-base-select-option", { hasText: "高级" }).click();
await drawer.getByTestId("save-settings").click();
await drawer.getByTestId("settings-state").filter({ hasText: "所有修改已保存" }).waitFor();
await page.keyboard.press("Escape");
await drawer.waitFor({ state: "hidden" });
const stale = answerOf("my_move").getByTestId("coach-answer-stale");
await stale.waitFor();
check((await stale.innerText()).includes("设置已改变") && (await stale.innerText()).includes("重新回答"), "对比面板保持打开时修改设置，回答立即标为过期，可以重新回答");
await oldRead.release();
check((await stale.count()) === 1, "设置改变前读取的旧状态晚到，不会把回答恢复为当前有效");
check((await askArea.getByTestId("coach-ask-my_move").count()) === 0, "过期的问题在回答卡片中重新回答，不在上方重复列出");
const callsBefore = (await calls()).length;
await stale.getByRole("button", { name: "重新回答" }).click();
let renewed = null;
for (let i = 0; i < 100 && !renewed; i++) {
  renewed = (await bridge("coach_answers", { req: pair })).find((a) => a.question.kind === "my_move" && a.status === "ready" && a.result_current);
  if (!renewed) await sleep(200);
}
await answerOf("my_move").getByTestId("coach-answer-text").waitFor();
check(
  !!renewed && (await calls()).length === callsBefore + 1 && (await answerOf("my_move").getByTestId("coach-answer-stale").count()) === 0,
  "重新回答时按新设置调用模型，回答不再标为过期",
);
await snap(page, "coach-answers");

await bridge("update_config", { config: { ...(await bridge("app_status")).config, coach: original } });
await fetch(`${LLM}/__reset`, { method: "POST" });

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
