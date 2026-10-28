// 场景 19：整局报告与专题训练。
// A. 本局报告：程序选出的关键局面写明依据（练习记录、收藏），值得保留的判断，说明评价的口径；
//    可以在棋盘上查看、加入复习；先想后讲中不能打开报告（会透露尚未揭示的着法）；
// B. 专题：卡片附程序根据规则事实建议的主题与依据；用户标记“我当时的问题”；
//    同一个问题标记到两张卡片时推荐练习并写明依据；按专题练一组；
// C. 报告属于打开时的那盘棋：先打开的另一盘棋的报告晚到，不覆盖当前的报告，查看时打开报告所属的会话；
// D. “全部加入复习”整批属于开始时的那份报告：中途关闭报告、换到另一盘棋并打开它的报告，后续的卡片仍加入原来那盘棋。
// 引擎的评价会随环境变化，这里只核对由练习记录与收藏决定的部分。
import { BASE, bridge, check, hold, launch, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const name = `整局报告 ${Date.now() % 100000}`;
const created = await bridge("create_study", {
  req: { name, kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8 3. 车一平二 马2进3" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 200; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 6) break;
  await sleep(500);
}
for (const i of await bridge("list_reviews")) await bridge("delete_review", { req: { id: i.card.id } });
// 先想后讲：第 2 着用了两级提示、走了马2进3（试走分支）；第 4 着走对；收藏第 3 着
await bridge("apply_move", { req: { study_id: sid, parent: 1, mv: "b9c7", variation: true } });
const practice = (node, hints, choice) =>
  bridge("record_practice", { req: { study_id: sid, node_id: node, hints_used: hints, revealed_directly: false, first_choice: choice, hints: null } });
await practice(2, 2, "b9c7");
await practice(4, 0, "i9h9");
await bridge("set_note", { req: { study_id: sid, node_id: 3, note: "", starred: true } });

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const report = page.getByTestId("report-dialog");
const keyOf = (node) => report.locator(`[data-report-node="${node}"]`);

// ---------- A. 本局报告 ----------
await page.getByTestId("report-open").click();
await report.getByTestId("report-overview").waitFor();
check((await report.getByTestId("report-overview").innerText()).includes("主线 6 着"), "概况：主线着法数与完整分析的着法数");
const second = await keyOf(2).innerText();
check(second.includes("第 1 回合 · 黑方 马8进7") && second.includes("先想后讲：用了 2 级提示") && second.includes("马2进3"), `练习中用了提示、选择与棋谱不同的局面入选：${second.replace(/\n/g, " / ")}`);
check((await keyOf(3).innerText()).includes("收藏"), "收藏的局面入选");
check((await report.getByTestId("report-keys").locator("li").count()) <= 5, "最多五处");
check((await report.getByTestId("report-good").innerText()).includes("先想后讲中第一步与棋谱一致：车9平8"), "值得保留的判断");
check((await report.getByTestId("report-notes").innerText()).includes("评价只比较每一着自己的候选"), "说明评价的口径");
await snap(page, "report");
await keyOf(3).getByTestId("report-add").click();
await keyOf(3).getByText("复习中 · 待复习").waitFor();
await keyOf(2).getByTestId("report-add").click();
await keyOf(2).getByText("复习中 · 待复习").waitFor();
check((await bridge("list_reviews")).length === 2, "从报告加入复习");
await keyOf(3).getByRole("button", { name: "在棋盘上查看" }).click();
await report.waitFor({ state: "hidden" });
await page.locator('[data-node="3"].active').waitFor();
check(true, "在棋盘上查看这一处局面");
await page.getByText("先想后讲", { exact: true }).click();
check(await page.getByTestId("report-open").isDisabled(), "先想后讲中不能打开报告");
await page.getByText("看谱学习", { exact: true }).click();

// ---------- B. 专题 ----------
const dialog = page.getByTestId("review-dialog");
await page.getByTestId("open-review").click();
await dialog.getByTestId("review-filter-all").click();
async function revealCard(node) {
  const id = `${sid}/${String(node).padStart(10, "0")}`;
  await dialog.locator(`[data-review="${id}"]`).click();
  await dialog.locator(`[data-review="${id}"].active`).waitFor();
  await dialog.getByTestId("review-reveal").click();
  await dialog.getByTestId("review-result").waitFor();
  return id;
}
const third = await revealCard(3);
const themes = await dialog.getByTestId("review-themes").innerText();
check(themes.includes("保护关系变化：这一步保护了"), `程序建议的主题附依据：${themes.replace(/\n/g, " / ")}`);
check((await dialog.getByTestId("review-recommend").count()) === 0, "还没有标记问题时不推荐");
await dialog.getByTestId("review-issue-missed_protection").click();
await dialog.getByTestId("review-issue-missed_protection").and(page.locator(".n-button--primary-type")).waitFor();
check((await bridge("list_reviews")).find((i) => i.card.id === third).card.issues.join() === "missed_protection", "保存“我当时的问题”");
await revealCard(2);
await dialog.getByTestId("review-issue-missed_protection").click();
const recommend = dialog.getByTestId("review-recommend");
await recommend.waitFor();
check((await recommend.innerText()).includes("你有 2 张卡片标记了“没有注意保护关系”"), "同一个问题标记到两张卡片时推荐练习，并写明依据");
await snap(page, "topics");
await recommend.getByTestId("review-recommend-start").click();
const progress = dialog.getByTestId("review-session-progress");
await progress.waitFor();
check((await progress.innerText()).includes("专题：我标记的：没有注意保护关系 · 第 1 / 2 张"), "练这一组标记了同一问题的卡片");
await dialog.getByTestId("review-finish").click();
await dialog.getByTestId("review-close-summary").click();
// 按程序建议的主题练一组
await dialog.getByTestId("review-topic").click();
const option = page.locator(".n-base-select-option", { hasText: "保护关系变化" });
await option.waitFor();
const optionText = await option.innerText();
await option.click();
await dialog.getByTestId("review-start-topic").click();
await progress.waitFor();
check((await progress.innerText()).startsWith("专题：保护关系变化"), `按主题练一组：${optionText}`);
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });

// ---------- C. 报告属于打开时的那盘棋 ----------
const otherName = `报告乙 ${Date.now() % 100000}`;
const other = await bridge("create_study", { req: { name: otherName, kind: { type: "record", text: "1. 兵七进一 卒7进1" }, orientation: null, user_side: null } });
// 打开这盘棋（甲）的报告并拦住响应；关闭后切换到乙、打开乙的报告，乙的报告返回后再放行甲的
const late = await hold(page, "game_report", { match: (body) => body.id === sid });
await page.getByTestId("report-open").click();
await late.arrived;
await page.keyboard.press("Escape");
await report.waitFor({ state: "hidden" });
await page.getByRole("button", { name: "新建 / 打开" }).click();
await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
await page.locator(".saved-row", { hasText: otherName }).locator(".saved-main").click();
await page.locator('[data-node="1"] .notation', { hasText: "兵七进一" }).waitFor();
await page.getByTestId("report-open").click();
await report.getByTestId("report-overview").filter({ hasText: otherName }).waitFor();
await late.release();
check(
  (await report.getByTestId("report-overview").innerText()).includes(otherName) && (await report.locator(".loading").count()) === 0,
  "先打开的另一盘棋的报告晚到，不覆盖当前这盘棋的报告",
);
check((await bridge("game_report", { id: other.study.study.id })).study_id === other.study.study.id, "报告按会话取得");
await page.keyboard.press("Escape");
await report.waitFor({ state: "hidden" });

// ---------- D. 全部加入复习属于开始时的那份报告 ----------
async function openSaved(studyName, firstMove) {
  await page.getByRole("button", { name: "新建 / 打开" }).click();
  await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
  await page.locator(".saved-row", { hasText: studyName }).locator(".saved-main").click();
  await page.locator('[data-node="1"] .notation', { hasText: firstMove }).waitFor();
}
const cardsOf = async (id) => (await bridge("list_reviews")).filter((i) => i.card.study_id === id).length;
for (const i of await bridge("list_reviews")) await bridge("delete_review", { req: { id: i.card.id } });
await openSaved(name, "炮二平五");
const adds = [];
const onRequest = (r) => {
  if (r.url().includes("/__bridge/invoke/add_review")) adds.push(r.postDataJSON().req);
};
page.on("request", onRequest);
// 拦住甲的第一条加入复习的响应；关闭报告、换到乙并打开乙的报告后再放行
const first = await hold(page, "add_review", { match: (body) => body.req.study_id === sid });
await page.getByTestId("report-open").click();
await report.getByTestId("report-overview").filter({ hasText: name }).waitFor();
const keys = await report.locator("[data-report-node]").count();
await report.getByTestId("report-add-all").click();
await first.arrived;
await page.keyboard.press("Escape");
await report.waitFor({ state: "hidden" });
await openSaved(otherName, "兵七进一");
await page.getByTestId("report-open").click();
await report.getByTestId("report-overview").filter({ hasText: otherName }).waitFor();
await first.release();
for (let i = 0; i < 50 && (await cardsOf(sid)) < keys; i++) await sleep(200);
page.off("request", onRequest);
check(
  adds.length === keys && adds.every((r) => r.study_id === sid) && (await cardsOf(sid)) === keys,
  `整批属于开始时的那盘棋：${adds.map((r) => `${r.study_id === sid ? "甲" : "乙"}/${r.node_id}`).join("、")}`,
);
check((await cardsOf(other.study.study.id)) === 0 && (await report.getByTestId("report-overview").innerText()).includes(otherName), "另一盘棋的复习卡片与报告不受影响");
await page.keyboard.press("Escape");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
