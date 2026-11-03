// 场景 22：复习中的分级提示、把握与用时，以及学习事件。
// A. 复习时可以看提示（与先想后讲相同，只依据局面）：二级提示在棋盘上标出相关的棋子；可以选择这一步的把握；
// B. 看了提示之后选对，记为“看了提示后选对”，标为已理解时不升档；揭示后写明看了几级提示、把握与用时；
// C. 每次作答记一条学习事件：结果、提示级数、把握与用时都在其中，重复的作答请求只记一条。
import { BASE, bridge, check, launch, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const created = await bridge("create_study", {
  req: { name: "提示与把握", kind: { type: "record", text: "1. 炮二平五 马8进7" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 2) break;
  await sleep(500);
}
for (const i of await bridge("list_reviews")) await bridge("delete_review", { req: { id: i.card.id } });
const earlier = await bridge("current_practice");
if (earlier) await bridge("update_practice", { req: { id: earlier.id, index: null, skip: null, goal: null, finish: true } });
const id = (await bridge("add_review", { req: { study_id: sid, node_id: 2 } })).card.id;
const before = (await bridge("learning_events")).length;

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
await page.getByTestId("open-review").click();
const dialog = page.getByTestId("review-dialog");
await dialog.locator(`[data-review="${id}"]`).click();
const reviewCard = dialog.getByTestId("review-card");
await dialog.getByTestId("review-prompt").waitFor();
await dialog.getByTestId("review-goal-recall").click();

// A. 提示与把握
await dialog.getByTestId("review-confidence-sure").click();
await dialog.getByTestId("review-hint").click();
const hints = dialog.getByTestId("review-hint-list");
await hints.filter({ hasText: "提示 1 · 局面需求" }).waitFor();
check(!(await hints.innerText()).includes("马8进7"), "提示不透露棋谱中的着法");
await dialog.getByTestId("review-hint").click();
await hints.filter({ hasText: "提示 2 · 相关棋子" }).waitFor();
const marked = await reviewCard.locator("circle.mark").count();
check(marked > 0, `二级提示在棋盘上标出相关的棋子（${marked} 处）`);
check((await dialog.getByTestId("review-hint").innerText()).includes("已看 2 级"), "按钮写明已经看了几级提示");
await snap(page, "review-hints");

// B. 走出棋谱中的着法：看了提示之后选对
for (const sq of ["h9", "g7"]) await reviewCard.locator(`[data-square="${sq}"]`).first().click();
await dialog.getByTestId("review-result").waitFor();
check((await dialog.getByTestId("review-badge").innerText()) === "看了提示后选对", "看了提示之后选对不算独立选对");
const notes = await dialog.getByTestId("review-attempt-notes").innerText();
check(/^看了 2 级提示 · 选择时很有把握 · 用时 \d+ 秒$/.test(notes), `揭示后写明提示、把握与用时：${notes}`);
await snap(page, "review-assisted");
await dialog.getByTestId("review-mark-understood").click();
await dialog.getByTestId("review-status").filter({ hasText: "已理解" }).waitFor();
const card = (await bridge("list_reviews")).find((i) => i.card.id === id).card;
check(card.stage === 0, `标为已理解时不升档（第 ${card.stage} 档）`);

// C. 学习事件
const events = await bridge("learning_events");
const mine = events.filter((e) => e.card === id);
check(events.length === before + 1 && mine.length === 1, `这次作答记了一条学习事件（共 ${events.length} 条）`);
const e = mine[0];
check(
  e.outcome === "assisted" && e.hints === 2 && e.confidence === "sure" && e.elapsed_ms > 0 && e.source === "review" && e.goal === "recall",
  `事件中有结果、提示级数、把握与用时：${JSON.stringify({ outcome: e.outcome, hints: e.hints, confidence: e.confidence, elapsed_ms: e.elapsed_ms })}`,
);

// 再试一次：提示与把握从头开始，直接看答案也记一条
await dialog.getByRole("button", { name: "再试一次" }).click();
await dialog.getByTestId("review-prompt").waitFor();
check((await dialog.getByTestId("review-hint").innerText()) === "看提示" && (await dialog.getByTestId("review-hint-list").count()) === 0, "再试一次时提示从头开始");
await dialog.getByTestId("review-reveal").click();
await dialog.getByTestId("review-result").waitFor();
const after = (await bridge("learning_events")).filter((x) => x.card === id);
check(after.length === 2 && after.some((x) => x.outcome === "revealed" && x.hints === 0 && x.confidence === null), "直接看答案也记一条，没有提示与把握");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
