// 场景 15：收藏、学习卡片与复习。
// A. 先想后讲揭示后加入复习：卡片保存落子前的局面、此前的着法、第一次选择、看过的提示、棋谱着法与程序讲解；
// B. 复习时先只显示局面（列表与提示都不透露答案），在棋盘上重新选择后，再显示这次的结果、原来的思考与讲解，
//    记下自己的理解，标为已理解；
// C. 把收藏的着法加入复习，直接看答案、标为需要再练、再试一次；
// D. 重新打开后卡片仍在，讲解面板显示复习状态，从卡片打开原局面；
// E. 原会话删除后卡片仍可复习，不能打开原局面；删除卡片。
import { BASE, bridge, check, launch, move, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const created = await bridge("create_study", {
  req: { name: "复习练习", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 4) break;
  await sleep(500);
}
// 收藏第 4 着并写下疑问（在界面中收藏需要先选中这一着）
await bridge("set_note", { req: { study_id: sid, node_id: 4, note: "车9平8 的作用是什么？", starred: true } });
// 清理之前运行留下的卡片
for (const i of await bridge("list_reviews")) await bridge("delete_review", { req: { id: i.card.id } });

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const dialog = page.getByTestId("review-dialog");
const reviewCard = dialog.getByTestId("review-card");
/** 在复习卡片的棋盘上走子 */
async function reviewMove(mv) {
  for (const sq of [mv.slice(0, 2), mv.slice(2, 4)]) await reviewCard.locator(`[data-square="${sq}"]`).first().dispatchEvent("mousedown", { button: 0 });
}
/** 顶部“复习”入口说明的待复习张数（数字角标的文字带有动画，读按钮的说明） */
const todoCount = async () => {
  const label = (await page.getByTestId("open-review").getAttribute("aria-label")) ?? "";
  return Number(label.match(/(\d+) 张待复习/)?.[1] ?? 0);
};
const cards = async () => bridge("list_reviews");
/** 在列表中选择一张卡片，等它成为当前复习的卡片 */
async function pick(id) {
  await dialog.locator(`[data-review="${id}"]`).click();
  await dialog.locator(`[data-review="${id}"].active`).waitFor();
  await dialog.getByTestId("review-prompt").waitFor();
}

// ---------- A. 先想后讲后加入复习 ----------
await page.getByText("先想后讲", { exact: true }).click();
await page.locator('[data-node="2"]').click();
await page.getByTestId("think-card").waitFor();
await page.getByTestId("hint-next").click();
await page.getByTestId("hint-next").click();
await move(page, "b9c7");
await page.getByTestId("practice").waitFor({ timeout: 30000 });
check((await page.getByTestId("practice").innerText()).includes("用了 2 级提示"), "揭示后显示练习记录");
await page.getByTestId("add-review").click();
await page.getByTestId("in-review").filter({ hasText: "复习中 · 待复习" }).waitFor();
check(true, "加入复习后显示“复习中 · 待复习”");
check((await todoCount()) === 1 && (await page.locator(".n-badge-sup").first().isVisible()), "顶部“复习”显示待复习的张数");
let list = await cards();
const card = list[0]?.card;
const before = (await bridge("node_detail", { req: { study_id: sid, node_id: 1 } })).node.fen.split(" ")[0];
check(list.length === 1 && card.fen.startsWith(before) && card.fen.endsWith(" b"), "卡片保存落子前的局面与行棋方");
check(card.history.map((m) => m.notation).join() === "炮二平五" && card.answer.notation === "马8进7", "卡片保存此前的着法与棋谱着法");
check(
  card.thinking?.first_choice?.notation === "马2进3" && card.thinking.hints_used === 2 && !!card.thinking.hints?.focus && card.thinking.hints.moves.length === 0,
  "卡片保存第一次选择与看过的两级提示原文",
);
check(!!card.basis?.summary && card.basis.generator_version > 0 && !!card.basis.conditions, "卡片保存程序讲解与分析版本");

// ---------- B. 复习：先选择，再对照 ----------
await page.getByTestId("open-review").click();
await reviewCard.waitFor();
await dialog.getByTestId("review-prompt").waitFor();
let text = await dialog.innerText();
check(!text.includes("马8进7") && !text.includes("马2进3"), "复习开始前不显示棋谱着法与原来的选择");
check(text.includes("轮到黑方走") && (await dialog.getByTestId("review-history").innerText()) === "此前：炮二平五", "先只给出局面、行棋方与此前的着法");
check((await reviewCard.locator(".xq-board line.arrow").count()) === 0, "复习开始前棋盘上没有箭头");
// 复习时方向键不移动主棋谱
await page.keyboard.press("ArrowLeft");
check((await page.locator('[data-node="2"].active').count()) === 1, "复习窗口打开时方向键不移动主棋谱");
await sleep(400); // 等对话框的淡入结束再截图
await snap(page, "review-choose");
await reviewMove("h9g7");
await dialog.getByTestId("review-result").waitFor();
check((await dialog.getByTestId("review-result").innerText()).includes("与棋谱相同"), "重新选择后显示这次的结果");
check(
  (await dialog.getByTestId("review-thinking").innerText()) === "当时用了 2 级提示，第一步走了 马2进3，与棋谱不同。",
  "显示原来的思考",
);
const hints = await dialog.getByTestId("review-hints").innerText();
check(hints.includes("提示 1 · 局面需求") && hints.includes("提示 2 · 相关棋子") && !hints.includes("提示 3"), "只显示当时看过的两级提示");
check((await dialog.getByTestId("review-answer").innerText()) === "马8进7" && (await dialog.getByTestId("review-summary").innerText()).length > 0, "显示棋谱着法与程序讲解");
check((await dialog.getByTestId("review-version").innerText()).includes("讲解规则第"), "注明讲解规则版本与分析条件");
check((await reviewCard.locator(".xq-board line.arrow").count()) === 1, "棋盘上标出棋谱中的着法");
check(await dialog.getByTestId("review-mark-understood").evaluate((el) => el.classList.contains("n-button--primary-type")), "走对时建议标为已理解");
await dialog.getByTestId("review-note").locator("textarea").fill("马8进7 保护中卒，同时出动左马");
await dialog.locator(".head .title").click();
await dialog.getByTestId("review-note-state").filter({ hasText: "已保存" }).waitFor();
await snap(page, "review-revealed");
await dialog.getByTestId("review-mark-understood").click();
await dialog.getByTestId("review-status").filter({ hasText: "已理解" }).waitFor();
list = await cards();
check(list[0].card.status === "understood" && list[0].card.note === "马8进7 保护中卒，同时出动左马" && list[0].card.attempts.length === 1, "状态、理解与这次复习都已保存");
check((await todoCount()) === 0, "没有待复习的卡片时不显示数字");

// ---------- C. 收藏的着法加入复习 ----------
await dialog.getByTestId("review-add-starred").click();
await dialog.locator("[data-review]").first().waitFor();
check((await dialog.getByTestId("review-filter-todo").innerText()).includes("1"), "收藏的着法加入后有 1 张待复习");
await pick(`${sid}/0000000004`);
check((await dialog.getByTestId("review-history").innerText()) === "此前：炮二平五 马8进7 马二进三", "第 4 着的此前着法");
await dialog.getByTestId("review-reveal").click();
await dialog.getByTestId("review-result").waitFor();
check((await dialog.getByTestId("review-result").innerText()).includes("直接查看了答案：棋谱中是 车9平8"), "不走子直接看答案");
check((await dialog.getByTestId("review-thinking").innerText()).includes("没有先想后讲的记录"), "收藏的着法没有练习记录时如实说明");
check((await dialog.getByTestId("review-note").locator("textarea").inputValue()) === "车9平8 的作用是什么？", "加入时带入这一步的笔记");
check(await dialog.getByTestId("review-mark-again").evaluate((el) => el.classList.contains("n-button--primary-type")), "没有走对时建议标为需要再练");
await dialog.getByTestId("review-mark-again").click();
await dialog.getByTestId("review-status").filter({ hasText: "需要再练" }).waitFor();
await dialog.getByRole("button", { name: "再试一次" }).click();
await dialog.getByTestId("review-prompt").waitFor();
await reviewMove("b9c7");
await dialog.getByTestId("review-result").filter({ hasText: "这次走了 马2进3，棋谱中是 车9平8" }).waitFor();
check((await reviewCard.locator(".xq-board line.arrow").count()) === 2, "棋盘上同时标出棋谱着法与这次的选择");
check((await dialog.getByTestId("review-attempts").innerText()).includes("✗✗"), "显示最近几次复习的结果");
check((await todoCount()) === 1, "需要再练的卡片计入待复习");
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });

// ---------- D. 重新打开后仍在，从卡片打开原局面 ----------
await page.reload();
await page.waitForSelector(".xq-board");
await page.locator('[data-node="2"]').click();
await page.getByTestId("in-review").filter({ hasText: "复习中 · 已理解" }).waitFor();
check(true, "重新打开后讲解面板显示这一着的复习状态");
await page.getByTestId("in-review").click();
await dialog.locator(`[data-review="${sid}/0000000002"].active`).waitFor();
check((await dialog.getByTestId("review-prompt").count()) === 1, "从讲解面板打开这一着的卡片（已理解的卡片也能再练）");
await dialog.getByTestId("review-filter-todo").click();
await pick(`${sid}/0000000004`);
await dialog.getByTestId("review-open-source").click();
await dialog.waitFor({ state: "hidden" });
await page.locator('[data-node="4"].active').waitFor();
check((await page.getByTestId("move-name").innerText()).includes("车9平8"), "打开原局面后显示这一着的完整讲解");

// ---------- E. 原会话删除后仍可复习 ----------
const temp = await bridge("create_study", { req: { name: "临时会话", kind: { type: "record", text: "1. 兵七进一" }, orientation: null, user_side: null } });
const tid = temp.study.study.id;
for (let i = 0; i < 60; i++) {
  if ((await bridge("study_view", { id: tid })).stats.detailed >= 1) break;
  await sleep(500);
}
await bridge("add_review", { req: { study_id: tid, node_id: 1 } });
await bridge("delete_study", { id: tid });
await page.getByTestId("open-review").click();
await dialog.getByTestId("review-filter-all").click();
const orphan = dialog.locator(`[data-review="${tid}/0000000001"]`);
await orphan.waitFor();
check((await orphan.innerText()).includes("临时会话"), "原会话删除后卡片仍在列表中");
await pick(`${tid}/0000000001`);
check(await dialog.getByTestId("review-open-source").isDisabled(), "原会话不在时不能打开原局面");
await dialog.getByTestId("review-reveal").click();
check((await dialog.getByTestId("review-answer").innerText()) === "兵七进一", "仍可按卡片复习");
await dialog.getByRole("button", { name: "删除卡片" }).click();
await page.locator(".n-popconfirm").getByRole("button", { name: "确认" }).click();
await orphan.waitFor({ state: "detached" });
check((await cards()).length === 2, "删除卡片");
await snap(page, "review-list");

// ---------- F. 迟到的响应与并发保存 ----------
const A = `${sid}/0000000002`;
const B = `${sid}/0000000004`;
/** 拦住某个命令的响应，直到调用返回的函数才交给页面（请求本身照常到达后台） */
async function holdResponse(cmd, match = () => true) {
  let release;
  let delivered;
  const released = new Promise((r) => (release = r));
  const done = new Promise((r) => (delivered = r));
  await page.route(`**/__bridge/invoke/${cmd}`, async (route) => {
    if (!match(route.request().postDataJSON())) return route.continue();
    const response = await route.fetch();
    await released;
    await route.fulfill({ response });
    delivered();
  });
  // 放行后等响应确实交给页面、界面处理完，再检查
  return async () => {
    release();
    await done;
    await sleep(300);
    await page.unroute(`**/__bridge/invoke/${cmd}`);
  };
}
// 卡片 A 的作答响应迟到：期间换到卡片 B，B 仍在等待选择，不显示 A 的结果
await pick(A);
let release = await holdResponse("review_attempt");
await reviewMove("h9g7");
await pick(B);
await release();
check(
  (await dialog.getByTestId("review-prompt").isVisible()) && (await dialog.getByTestId("review-result").count()) === 0,
  "换到另一张卡片后，迟到的作答结果不会揭示这张卡片",
);
check((await dialog.locator(`[data-review="${B}"].active`).count()) === 1, "当前仍是后打开的卡片");
check((await cards()).find((i) => i.card.id === A).card.attempts.length === 2, "迟到的作答仍记在原来的卡片上");
// 两张卡片的详情倒序返回：最后打开的卡片保持为当前卡片
release = await holdResponse("review_detail", (body) => body.req.id === A);
await dialog.locator(`[data-review="${A}"]`).click();
await sleep(200);
await dialog.locator(`[data-review="${B}"]`).click();
// 后打开的卡片先加载完成，再放行先打开的那张：两次详情的响应倒序到达
await dialog.locator(`[data-review="${B}"].active`).waitFor();
await release();
check(
  (await dialog.locator(`[data-review="${B}"].active`).count()) === 1 && (await dialog.getByTestId("review-history").innerText()) === "此前：炮二平五 马8进7 马二进三",
  "先打开的卡片详情较晚返回时，不会跳回那张卡片",
);
// 保存理解与标记状态：标记状态等理解保存完成后再发送，两者都保留
const order = [];
await page.route("**/__bridge/invoke/update_review", async (route) => {
  const kind = route.request().postDataJSON().req.note !== null ? "note" : "status";
  order.push(`${kind}:发出`);
  if (kind === "note") await sleep(800);
  const response = await route.fetch();
  order.push(`${kind}:完成`);
  await route.fulfill({ response });
});
await dialog.getByTestId("review-reveal").click();
await dialog.getByTestId("review-result").waitFor();
await dialog.getByTestId("review-note").locator("textarea").fill("车9平8 出车，准备过河");
await dialog.getByTestId("review-mark-understood").click();
await dialog.getByTestId("review-status").filter({ hasText: "已理解" }).waitFor();
await page.unroute("**/__bridge/invoke/update_review");
check(order.join() === "note:发出,note:完成,status:发出,status:完成", `同一张卡片的修改依次发送：${order.join("，")}`);
const saved = (await cards()).find((i) => i.card.id === B).card;
check(saved.note === "车9平8 出车，准备过河" && saved.status === "understood", "理解与状态都保存下来");

// ---------- 窄窗口 ----------
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });
await page.setViewportSize({ width: 760, height: 900 });
await page.getByTestId("open-review").click();
await reviewCard.locator(".xq-board").waitFor();
const fits = await dialog.evaluate((el) => {
  const card = el.querySelector(".n-card__content") ?? el;
  return card.scrollWidth <= card.clientWidth + 1 && el.getBoundingClientRect().right <= window.innerWidth + 1;
});
check(fits && (await dialog.getByTestId("review-list").isVisible()), "窄窗口下卡片列表与棋盘上下排列，没有横向溢出");
await sleep(400);
await snap(page, "review-narrow");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
