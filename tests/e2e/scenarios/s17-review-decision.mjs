// 场景 17：复习的练习目标与意图。
// A. 默认复现棋谱；切换为局面决策后记住选择；落子前可以选择意图；
// B. 局面决策：比较在后台进行，完成后显示四种结论之一、对照着法与分析条件；意图与这一步实际的规则事实对照；
//    只在留下值得注意的问题时建议再练，与棋谱一致或接近时提醒对照意图与实际变化后自己判断；
// C. 与棋谱一致也按局面决策显示，不自动建议已理解；复现棋谱沿用是否与棋谱一致的建议；最近几次复习的记号区分两种目标。
// 引擎给出的评分会随环境变化，这里只核对确定的部分（与棋谱一致、规则事实、显示的结论属于四种之一）。
import { BASE, bridge, check, launch, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const created = await bridge("create_study", {
  req: { name: "局面决策复习", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 2) break;
  await sleep(500);
}
for (const i of await bridge("list_reviews")) await bridge("delete_review", { req: { id: i.card.id } });
const card = (await bridge("add_review", { req: { study_id: sid, node_id: 2 } })).card;
const cardOf = async () => (await bridge("list_reviews")).find((i) => i.card.id === card.id).card;

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const dialog = page.getByTestId("review-dialog");
const reviewCard = dialog.getByTestId("review-card");
async function reviewMove(mv) {
  for (const sq of [mv.slice(0, 2), mv.slice(2, 4)]) await reviewCard.locator(`[data-square="${sq}"]`).first().dispatchEvent("mousedown", { button: 0 });
}
async function openCard() {
  await page.getByTestId("open-review").click();
  await dialog.getByTestId("review-filter-all").click();
  await dialog.locator(`[data-review="${card.id}"]`).click();
  await dialog.locator(`[data-review="${card.id}"].active`).waitFor();
  await dialog.getByTestId("review-prompt").waitFor();
}
const checked = (id) => dialog.getByTestId(id).evaluate((el) => el.classList.contains("n-radio-button--checked"));
const isPrimary = (id) => dialog.getByTestId(id).evaluate((el) => el.classList.contains("n-button--primary-type"));
/** 等局面决策的比较完成，返回显示的结论 */
async function decided() {
  const el = dialog.getByTestId("review-decision");
  await el.waitFor();
  for (let i = 0; i < 300; i++) {
    const t = (await el.innerText()).trim();
    if (!t.includes("正在用同一次搜索比较")) return t;
    await sleep(200);
  }
  throw new Error("局面决策的比较没有完成");
}
const VERDICTS = ["与棋谱一致", "与棋谱不同，但本次分析中与参考评分接近", "这个选择留下了一个值得注意的问题", "分析尚不充分，暂不评价走法优劣"];

// ---------- A. 练习目标与意图 ----------
await openCard();
check(await checked("review-goal-recall"), "默认的练习目标是复现棋谱");
await dialog.getByTestId("review-goal-decision").click();
check((await dialog.getByTestId("review-goal").locator("..").innerText()).includes("与棋谱着法、引擎参考在同一次搜索中比较"), "说明局面决策比较什么");
await dialog.getByTestId("review-intent-develop").click();
check(await isPrimary("review-intent-develop"), "落子前选择意图：出子");
await snap(page, "decision-choose");

// ---------- B. 局面决策 ----------
// 马2进3（与棋谱中的马8进7不同）
await reviewMove("b9c7");
await dialog.getByTestId("review-result").waitFor();
const verdict = await decided();
check(VERDICTS.includes(verdict) || verdict.startsWith("没有得到比较结果"), `比较完成后显示结论：${verdict}`);
const points = dialog.getByTestId("review-decision-points");
if (verdict !== "与棋谱一致" && !verdict.startsWith("没有得到")) {
  check((await points.innerText()).length > 0, `结论附带程序给出的说明：${(await points.innerText()).replace(/\n/g, " / ")}`);
}
check((await dialog.getByText("分析条件：").count()) > 0 && (await dialog.locator(".conditions", { hasText: "指定着法单独分析" }).count()) > 0, "注明比较使用的分析条件");
check((await dialog.getByTestId("review-intent-title").innerText()).includes("你原来想：出子"), "揭示后显示原来的意图");
check((await dialog.getByTestId("review-intent-checks").innerText()).includes("这一步把右马从底线走出"), "意图与这一步实际的规则事实对照");
check((await dialog.getByTestId("review-changes").locator("li").count()) > 0, "列出这一步实际改变了什么");
if (verdict === "这个选择留下了一个值得注意的问题") {
  check(await isPrimary("review-mark-again"), "留下值得注意的问题时建议再练");
} else {
  check(
    !(await isPrimary("review-mark-again")) && !(await isPrimary("review-mark-understood")) && (await dialog.getByTestId("review-self-check").isVisible()),
    "其余结论不自动建议状态，提醒对照意图与实际变化后自己判断",
  );
}
let saved = (await cardOf()).attempts.at(-1);
check(saved.goal === "decision" && saved.intent === "develop" && saved.decision.state !== "pending", "这次复习保存了练习目标、意图与比较结果");
await snap(page, "decision-result");

// ---------- C. 与棋谱一致、复现棋谱 ----------
await dialog.getByRole("button", { name: "再试一次" }).click();
await dialog.getByTestId("review-prompt").waitFor();
check(await checked("review-goal-decision"), "再试一次时保留练习目标");
check(!(await isPrimary("review-intent-develop")), "再试一次时意图重新选择");
await dialog.getByTestId("review-intent-capture").click();
await reviewMove("h9g7");
check((await decided()) === "与棋谱一致", "局面决策中走出棋谱着法：与棋谱一致");
check((await dialog.getByTestId("review-intent-checks").innerText()).includes("这一步没有吃子"), "意图是吃子，规则事实说明这一步没有吃子");
check(
  !(await isPrimary("review-mark-understood")) && (await dialog.getByTestId("review-self-check").isVisible()),
  "与棋谱一致不自动建议已理解",
);
await dialog.getByRole("button", { name: "再试一次" }).click();
await dialog.getByTestId("review-goal-recall").click();
await reviewMove("h9g7");
await dialog.getByTestId("review-result").waitFor();
check((await dialog.getByTestId("review-decision").count()) === 0 && (await isPrimary("review-mark-understood")), "复现棋谱：走对时建议已理解，不做局面比较");
const marks = await dialog.getByTestId("review-attempts").innerText();
check(marks.includes("✓") && (marks.includes("○") || marks.includes("△") || marks.includes("·")), `最近几次复习的记号区分两种目标：${marks}`);

// 记住练习目标
await dialog.getByRole("button", { name: "再试一次" }).click();
await dialog.getByTestId("review-goal-decision").click();
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });
await page.reload();
await page.waitForSelector(".xq-board");
await openCard();
check(await checked("review-goal-decision"), "重新打开后记住上次的练习目标");
saved = await cardOf();
check(saved.attempts.length === 3 && saved.attempts.map((a) => a.goal).join() === "decision,decision,recall", "每次复习各自记下练习目标");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
