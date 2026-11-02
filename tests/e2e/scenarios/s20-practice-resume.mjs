// 场景 20：今日学习首页与可以中断后继续的专注练习。
// A. 首页：继续上次练习、今天到期、继续复盘三块，主要操作随状态决定；从首页两次点击以内开始一组练习；
// B. 局面决策的比较在后台进行时进入下一题：结果晚到后更新原来那次作答，不切换当前题目；
// C. 保存退出后回到首页；重新启动后从原处继续：顺序、当前题目、已经揭示的状态与练习目标都恢复；
// D. 练习中删除的卡片说明已跳过、继续后面的题目；同一次作答的请求重复到达只记一次，重复标记不重复升档；
// E. 窄窗口中主要操作仍在可见区域，可以用键盘操作；跳过与直接看答案分开记录，小结分别说明。
import { BASE, bridge, check, hold, launch, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const name = `中断后继续 ${Date.now() % 100000}`;
const created = await bridge("create_study", {
  req: { name, kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8 3. 车一平二" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 200; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 5) break;
  await sleep(500);
}
// 清理之前留下的卡片与进行中的练习
for (const i of await bridge("list_reviews")) await bridge("delete_review", { req: { id: i.card.id } });
const earlier = await bridge("current_practice");
if (earlier) await bridge("update_practice", { req: { id: earlier.id, index: null, skip: null, goal: null, finish: true } });
const ids = [];
for (const node of [1, 2, 3, 4, 5]) ids.push((await bridge("add_review", { req: { study_id: sid, node_id: node } })).card.id);
const cardOf = async (id) => (await bridge("list_reviews")).find((i) => i.card.id === id)?.card;
// 比较的搜索按时间结束（深度不设限），好在结果出来之前进入下一题
const original = (await bridge("app_status")).config;
await bridge("update_config", { config: { ...original, analysis: { ...original.analysis, quick_time_ms: 2000, quick_depth: 100 } } });

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const home = page.getByTestId("home");
const ws = page.getByTestId("practice-workspace");
const wsCard = ws.getByTestId("review-card");
const isPrimary = (el) => el.evaluate((e) => e.classList.contains("n-button--primary-type"));
async function wsMove(mv) {
  for (const sq of [mv.slice(0, 2), mv.slice(2, 4)]) await wsCard.locator(`[data-square="${sq}"]`).first().dispatchEvent("mousedown", { button: 0 });
}
const history = (text) => ws.getByTestId("review-history").filter({ hasText: text }).waitFor();
const entry = (i) => ws.locator(`[data-practice-entry="${i}"]`);

// ---------- A. 首页 ----------
await page.getByTestId("open-home").click();
await home.waitFor();
check((await home.getByTestId("home-today-count").innerText()) === "5 张卡片今天到期", "首页说明今天到期的张数");
check((await home.getByTestId("home-session").innerText()).includes("没有进行中的练习"), "没有进行中的练习时如实说明");
check(await isPrimary(home.getByTestId("home-start-today")), "没有进行中的练习、有到期的卡片时，主要操作是开始今天的练习");
check((await home.locator(`[data-home-study="${sid}"]`).innerText()).includes(name), "继续复盘列出最近的棋谱");
await snap(page, "home");
await home.getByTestId("home-start-today").click();
const progress = ws.getByTestId("practice-progress");
await progress.waitFor();
check((await progress.innerText()).includes("第 1 / 5 题"), "从首页两次点击开始一组练习，进入专注练习工作区");

// ---------- B. 比较结果晚到，不切换当前题目 ----------
await history("从开局第一着开始");
await ws.getByTestId("review-goal-decision").click();
await wsMove("h2e2");
await ws.getByTestId("review-result").waitFor();
const pendingAtSwitch = (await ws.getByTestId("review-decision").innerText()).includes("正在用同一次搜索比较");
check((await ws.getByTestId("review-badge").innerText()) === "比较中", "比较还没有结果时标为“比较中”，不当作选对或选错");
await snap(page, "practice-pending");
await ws.getByTestId("review-next").click();
await history("此前：炮二平五");
for (let i = 0; i < 100 && (await cardOf(ids[0])).attempts[0].decision.state === "pending"; i++) await sleep(200);
const decided = (await cardOf(ids[0])).attempts[0];
await sleep(300);
check(
  pendingAtSwitch && decided.decision.state !== "pending" && (await ws.getByTestId("review-history").innerText()) === "此前：炮二平五" && (await ws.getByTestId("review-prompt").isVisible()),
  "比较结果晚到：写回原来那次作答，当前仍是第 2 题",
);
await ws.getByTestId("practice-toggle-list").click();
const first = await entry(0).locator(".badge").innerText();
check(/^(比较支持|比较不支持|无法评价)$/.test(first), `本组卡片列表随结果更新：第 1 题 ${first}`);
// 第 2 题：改回复现棋谱，直接看答案
await ws.getByTestId("review-goal-recall").click();
await ws.getByTestId("review-reveal").click();
await ws.getByTestId("review-result").waitFor();
check((await ws.getByTestId("review-badge").innerText()) === "直接看了答案", "直接看答案记为一次作答");
await snap(page, "practice-revealed");

// ---------- C. 保存退出、重新启动后继续 ----------
await ws.getByTestId("practice-exit").click();
await home.waitFor();
const sessionText = await home.getByTestId("home-session-progress").innerText();
check(sessionText === "复现棋谱 · 已作答 2 / 5 题，还有 3 题" && (await isPrimary(home.getByTestId("home-resume"))), `保存退出后回到首页，主要操作是继续练习：${sessionText}`);
// 退出期间删除第 3 题的卡片
await bridge("delete_review", { req: { id: ids[2] } });
await page.reload();
await page.waitForSelector(".xq-board");
await page.getByTestId("open-home").click();
await home.getByTestId("home-resume").click();
await progress.waitFor();
check((await progress.innerText()).includes("第 2 / 5 题 · 已作答 2 题"), `重新启动后回到原来的题目：${await progress.innerText()}`);
check(
  (await ws.getByTestId("review-result").innerText()).includes("直接查看了答案") && (await ws.getByTestId("practice-goal").innerText()).startsWith("复现棋谱"),
  "已经揭示的题目恢复到揭示之后，练习目标也恢复",
);
await ws.getByTestId("practice-toggle-list").click();
const badges = [];
for (let i = 0; i < 5; i++) badges.push(await entry(i).locator(".badge").innerText());
check(badges[1] === "直接看了答案" && badges[2] === "卡片已删除" && badges[3] === "未作答", `本组卡片列表：${badges.join("、")}`);

// ---------- D. 删除的卡片已跳过；重复的请求只记一次；重复标记不重复升档 ----------
await ws.getByTestId("review-next").click();
await ws.getByTestId("practice-notice").filter({ hasText: "第 3 题的卡片已经删除，已跳过" }).waitFor();
await history("马二进三");
check((await progress.innerText()).includes("第 4 / 5 题"), "继续后面的题目");
let captured = null;
const late = hold(page, "review_attempt", {
  match: (body) => {
    captured = body;
    return body.req.id === ids[3];
  },
});
const held = await late;
await wsMove("i9h9");
await held.arrived;
// 同一次作答的请求再次到达（例如网络重试）：后台只返回已经记下的那次
const again = await bridge("review_attempt", captured);
await held.release();
await ws.getByTestId("review-result").waitFor();
const fourth = await cardOf(ids[3]);
const session = await bridge("current_practice");
check(
  again.card.attempts.length === 1 && fourth.attempts.length === 1 && fourth.attempts[0].practice === session.id && !!captured.req.request,
  "同一次作答的请求重复到达只记一次，作答记入这组练习",
);
await ws.getByTestId("review-mark-understood").click();
await ws.getByTestId("review-status").filter({ hasText: "已理解" }).waitFor();
await ws.getByTestId("review-mark-understood").click();
await sleep(500);
const marked = await cardOf(ids[3]);
check(marked.stage === 1 && marked.schedule_base.attempt_at === fourth.attempts[0].at, `重复标记同一次作答不重复升档（第 ${marked.stage} 档）`);

// ---------- E. 窄窗口与键盘；跳过；小结 ----------
await page.setViewportSize({ width: 720, height: 800 });
const next = ws.getByTestId("review-next");
// 操作区固定在可见区域底部：不用滚动就能看到
const box = await next.boundingBox();
check(!!box && box.y >= 0 && box.y + box.height <= 800 && (await isPrimary(next)), "窄窗口中标记之后的主要操作（下一题）在可见区域");
await snap(page, "practice-narrow");
await next.focus();
await page.keyboard.press("Enter");
await history("车9平8");
check((await progress.innerText()).includes("第 5 / 5 题"), "用键盘进入下一题");
await page.setViewportSize({ width: 1440, height: 900 });
await ws.getByTestId("review-skip").click();
const summary = await ws.getByTestId("review-session-summary").innerText();
check(
  summary.startsWith("本次完成 3 张：") && summary.includes("1 张直接看了答案") && summary.includes("跳过 1 张") && summary.includes("1 张卡片已删除"),
  `小结分别说明完成、直接看答案、跳过与已删除：${summary}`,
);
for (let i = 0; i < 25 && (await bridge("current_practice")) !== null; i++) await sleep(200);
check((await bridge("current_practice")) === null, "最后一题跳过后这组练习结束");
await snap(page, "practice-summary");
await ws.getByTestId("review-close-summary").click();
await home.waitFor();
check((await home.getByTestId("home-session").innerText()).includes("没有进行中的练习"), "完成后回到首页");
await home.getByTestId("home-board").click();
await page.waitForSelector(".xq-board");

await bridge("update_config", { config: original });
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
