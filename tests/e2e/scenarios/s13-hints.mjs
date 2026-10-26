// 场景 13：先想后讲的分级提示与训练状态。
// A. 训练状态：正在思考的这一着及其后续着法在整个界面上都不透露，局面分析与笔记暂不可用；
// B. 分级提示：一级只说局面需求，二级在棋盘上标出相关棋子，三级给出不分先后的着法，点一步可查看箭头；
// C. 练习记录：用了几级提示、是否直接看讲解、第一步是否与棋谱相同，随会话保存；
// D. 提示属于哪盘棋的哪个局面：换到另一盘棋的同一编号、起点还在加载时，都不沿用之前的提示。
import { BASE, bridge, check, hold, launch, move, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const created = await bridge("create_study", {
  req: { name: "先想后讲提示", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 4) break;
  await new Promise((r) => setTimeout(r, 500));
}

// AI 讲解使用模拟模型服务（只用于检查批量生成的列表不透露答案），结束时恢复原来的设置
const LLM = process.env.MOCK_LLM ?? "http://127.0.0.1:1440";
const original = (await bridge("app_status")).config.coach;
{
  const cfg = (await bridge("app_status")).config;
  const coach = { ...cfg.coach, enabled: true, provider: "deepseek", base_url: LLM, model: "mock-model", auto_explain_scope: "off", daily_request_budget: 0 };
  await bridge("update_config", { config: { ...cfg, coach } });
  await bridge("set_coach_credential", { req: { key: "sk-e2e-hints" } });
  await fetch(`${LLM}/__reset`, { method: "POST" });
}

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const notationOf = async (id) => (await page.locator(`[data-node="${id}"] .notation`).innerText()).trim();
const bodyText = () => page.evaluate(() => document.body.innerText);
const practiceOf = async (id) => (await bridge("study_view", { id: sid })).study.nodes[String(id)].practice;

// ---------- A. 训练状态 ----------
await page.getByText("先想后讲", { exact: true }).click();
await page.locator('[data-node="2"]').click();
await page.getByTestId("think-card").waitFor();
check((await notationOf(1)) === "炮二平五", "已经走过的着法照常显示");
check((await notationOf(2)).includes("？？？") && (await notationOf(3)).includes("？？？") && (await notationOf(4)).includes("？？？"), "正在思考的这一着及其后续着法在棋谱中隐藏");
check(
  (await page.locator(".n-tabs-tab--disabled", { hasText: "局面分析" }).count()) === 1 && (await page.locator(".n-tabs-tab--disabled", { hasText: "笔记" }).count()) === 1,
  "揭示前局面分析与笔记暂不可用",
);
let text = await bodyText();
check(!["马8进7", "马二进三", "车9平8"].some((n) => text.includes(n)), "整个界面上都没有透露这一着与后续着法");
check((await page.getByTestId("hint-count").innerText()).includes("0 / 3"), "开始时没有使用提示");
// 删除确认框与批量生成的列表同样不透露尚未揭示的着法
await page.locator('[data-node="3"]').hover();
await page.locator('[data-node="3"] .more').click();
await page.locator(".n-dropdown-option", { hasText: "删除此着及之后" }).click();
const confirm = page.locator(".n-dialog");
await confirm.waitFor();
const confirmText = await confirm.innerText();
check(!["马8进7", "马二进三", "车9平8"].some((n) => confirmText.includes(n)) && confirmText.includes("尚未揭示"), `删除确认框不显示尚未揭示的着法：${confirmText.replace(/\n/g, " ")}`);
await confirm.getByRole("button", { name: "取消" }).click();
await confirm.waitFor({ state: "hidden" });
await page.getByTestId("coach-batch-open").click();
const batchDialog = page.getByTestId("coach-batch");
await batchDialog.getByTestId("batch-scope-line").click();
await batchDialog.getByTestId("batch-summary").waitFor();
await sleep(300);
await batchDialog.getByTestId("batch-start").click();
await batchDialog.getByTestId("batch-phase").filter({ hasText: "全部完成" }).waitFor({ timeout: 60000 });
const batchText = await batchDialog.innerText();
check(
  !["马8进7", "马二进三", "车9平8"].some((n) => batchText.includes(n)) && batchText.includes("黑方 · 尚未揭示"),
  `批量生成的列表只显示回合与走棋方：${(await batchDialog.locator("[data-batch-node]").allInnerTexts()).join(" / ").replace(/\n/g, " ")}`,
);
await page.keyboard.press("Escape");
await batchDialog.waitFor({ state: "hidden" });
text = await bodyText();
check(!["马8进7", "马二进三", "车9平8"].some((n) => text.includes(n)), "生成 AI 讲解之后，界面上仍不透露这一着与后续着法");

// ---------- B. 分级提示 ----------
const next = page.getByTestId("hint-next");
check(await next.evaluate((el) => el.classList.contains("n-button--primary-type")), "“给我一点提示”是主要操作");
check((await next.innerText()).includes("给我一点提示"), "第一级提示的入口");
await next.click();
const hints = page.getByTestId("hints");
const level1 = await hints.innerText();
check(level1.includes("提示 1 · 局面需求") && !level1.includes("提示 2"), `一级提示只说局面需求：${level1.replace(/\n/g, " ")}`);
check((await page.locator(".xq-board circle.hl").count()) === 0, "一级提示不在棋盘上标出棋子");
const observe = (await hints.locator("li").first().locator("p").first().innerText()).trim();
// 练习期间分析更新、局面详情中的提示随之改变（这里拦截详情模拟）：已经看到的提示与之后的各级提示都不变
await page.route("**/__bridge/invoke/node_detail", async (route) => {
  const response = await route.fetch();
  const body = await response.json();
  if (body.hints) {
    body.hints.observe = "（分析更新后的提示）";
    body.hints.moves = [{ mv: "a3a4", notation: "兵九进一" }];
  }
  await route.fulfill({ response, json: body });
});
await page.locator('[data-node="2"]').click();
await sleep(800);
check(!(await hints.innerText()).includes("分析更新后"), "分析更新后，已经看到的提示不变");
await next.click();
const level2 = await hints.innerText();
check(level2.includes("提示 2 · 相关棋子"), `二级提示指出相关棋子：${level2.split("提示 2 · 相关棋子")[1]?.replace(/\n/g, " ").slice(0, 40)}`);
check((await page.locator(".xq-board circle.hl").count()) > 0, "二级提示在棋盘上标出相关棋子");
text = await bodyText();
check(!text.includes("马8进7"), "一、二级提示没有透露棋谱中的着法");
await snap(page, "hint-2");
await next.click();
const chips = page.locator(".hint-move");
check((await chips.count()) >= 1 && (await hints.innerText()).includes("不分先后"), `三级提示给出不分先后的着法：${(await chips.allInnerTexts()).join("、")}`);
const shownMoves = (await chips.allInnerTexts()).map((t) => t.trim());
check(!shownMoves.includes("兵九进一"), "三级提示来自第一次打开提示时的内容");
await page.unroute("**/__bridge/invoke/node_detail");
check((await page.getByTestId("hint-next").count()) === 0, "三级提示之后不再有更多提示");
await chips.first().click();
check((await page.locator(".xq-board line.arrow.next").count()) === 1, "点一步提示中的着法，在棋盘上显示它的走法");
check((await page.getByTestId("hint-count").innerText()).includes("3 / 3"), "记下用了三级提示");
await snap(page, "hint-3");

// ---------- C. 练习记录 ----------
// 走出与棋谱不同的一步（马2进3）
await move(page, "b9c7");
await page.getByTestId("practice").waitFor({ timeout: 30000 });
let p = await page.getByTestId("practice").innerText();
check(p.includes("用了 3 级提示") && p.includes("与棋谱不同"), `揭示后显示练习记录：${p}`);
check((await notationOf(2)) === "马8进7", "揭示后棋谱显示这一着");
check((await notationOf(3)).includes("？？？") && (await notationOf(4)).includes("？？？"), "这一着揭示后，后续着法仍然隐藏，等思考后再显示");
check((await page.locator(".variation .notation", { hasText: "马2进3" }).count()) === 1, "自己走出的试走着法照常显示");
let rec = await practiceOf(2);
check(rec?.hints_used === 3 && rec.first_choice === "b9c7" && rec.matched === false && !rec.revealed_directly, "练习记录保存在会话中");
check(
  rec?.hints?.observe === observe && rec.hints.moves.map((m) => m.notation).join() === shownMoves.join(),
  "练习记录保存的是练习时实际看到的提示",
);

// 不走子直接看讲解
await page.locator('[data-node="3"]').click();
await page.getByTestId("think-card").waitFor();
await page.getByTestId("reveal").click();
await page.getByTestId("practice").waitFor();
p = await page.getByTestId("practice").innerText();
check(p.includes("没有用提示") && p.includes("没有走子就看了讲解"), `直接看讲解也有记录：${p}`);
rec = await practiceOf(3);
check(rec?.revealed_directly === true && rec.first_choice === null, "记为直接揭示");

// 走出与棋谱相同的一步（车9平8）
await page.locator('[data-node="4"]').click();
await page.getByTestId("think-card").waitFor();
await move(page, "i9h9");
await page.getByTestId("practice").waitFor({ timeout: 30000 });
p = await page.getByTestId("practice").innerText();
check(p.includes("与棋谱相同"), `走对时记为与棋谱相同：${p}`);
check((await practiceOf(4))?.matched === true, "保存为与棋谱相同");

// 回到看谱学习：全部着法照常显示
await page.getByText("看谱学习", { exact: true }).click();
check((await notationOf(3)) === "马二进三" && (await notationOf(4)) === "车9平8", "回到看谱学习后棋谱照常显示");

// ---------- D. 提示属于哪盘棋的哪个局面 ----------
// 两盘不同的棋，都打开在第 2 着（节点编号相同）。第一盘的第 2 着前对方的炮可以被吃，第二盘没有直接的威胁或机会，
// 两处的提示不同
const tag = Date.now() % 100000;
async function newStudy(name, record) {
  const resp = await bridge("create_study", { req: { name, kind: { type: "record", text: record }, orientation: null, user_side: null } });
  const id = resp.study.study.id;
  for (let i = 0; i < 120; i++) {
    if ((await bridge("study_view", { id })).stats.detailed >= 2) break;
    await sleep(500);
  }
  return id;
}
/** 在界面中打开保存的会话（打开后停在最后一着），等这个局面的提示准备好 */
async function openSaved(name, firstMove) {
  await page.getByRole("button", { name: "新建 / 打开" }).click();
  await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
  await page.locator(".saved-row", { hasText: name }).locator(".saved-main").click();
  await page.locator('[data-node="1"] .notation', { hasText: firstMove }).waitFor();
  await page.locator('[data-node="2"].active').waitFor();
  await page.getByTestId("hint-count").waitFor();
}
const hintsOf = async (id, node) => (await bridge("node_detail", { req: { study_id: id, node_id: node } })).hints;
const firstHint = async () => (await page.getByTestId("hints").locator("li").first().locator("p").first().innerText()).trim();
const nameP = `提示归属甲 ${tag}`;
const nameQ = `提示归属乙 ${tag}`;
const pid = await newStudy(nameP, "1. 炮二进七 车9平8");
const qid = await newStudy(nameQ, "1. 兵七进一 卒7进1");
await page.getByText("先想后讲", { exact: true }).click();
await openSaved(nameP, "炮二进七");
// 换到第 1 着时，它的起点（开局局面）的详情晚到：期间不能把第 2 着起点的提示当作这一着的提示
const start = await hold(page, "node_detail", { match: (body) => body.req.study_id === pid && body.req.node_id === 0 });
await page.locator('[data-node="1"]').click();
await start.arrived;
await page.locator('[data-node="1"].active').waitFor();
check(
  (await page.getByTestId("hint-next").count()) === 0 && (await page.getByTestId("hints").count()) === 0,
  "起点详情还没有加载时等它加载，不提供上一个局面的提示",
);
await start.release();
await page.getByTestId("hint-next").click();
const observeStart = await firstHint();
const [startHints, beforeP] = [await hintsOf(pid, 0), await hintsOf(pid, 1)];
check(observeStart === startHints.observe && observeStart !== beforeP.observe, `起点加载后显示这个局面的提示：${observeStart}`);
// 回到第 2 着看一级提示，再打开另一盘棋：同样停在第 2 着
await page.locator('[data-node="2"]').click();
await page.getByTestId("hint-count").filter({ hasText: "0 / 3" }).waitFor();
await page.getByTestId("hint-next").click();
const observeP = await firstHint();
check(observeP === beforeP.observe, `第一盘棋第 2 着的一级提示：${observeP}`);
await openSaved(nameQ, "兵七进一");
check(
  (await page.getByTestId("hint-count").innerText()).includes("0 / 3") && (await page.getByTestId("hints").count()) === 0,
  "换到另一盘棋的同一编号，提示次数从零开始，不显示之前的提示",
);
await page.getByTestId("hint-next").click();
const observeQ = await firstHint();
check(observeQ === (await hintsOf(qid, 1)).observe && observeQ !== observeP, `显示的是这盘棋这个局面的提示：${observeQ}`);
await page.getByTestId("reveal").click();
await page.getByTestId("practice").waitFor();
const recQ = (await bridge("study_view", { id: qid })).study.nodes["2"].practice;
check(recQ?.hints_used === 1 && recQ.hints?.observe === observeQ, "这盘棋的练习记录保存的是它自己看到的提示");

await bridge("update_config", { config: { ...(await bridge("app_status")).config, coach: original } });

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
