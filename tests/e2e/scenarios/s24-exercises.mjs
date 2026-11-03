// 场景 24：从自己的棋局生成练习（模拟引擎）。整局报告中“从这盘棋生成练习”：按自己一方的着法出找威胁、二选一、
// 找更好的着法，写明新增几道；复习库中标出题型；找威胁在棋盘上点出受到威胁的棋子后提交，二选一用按钮选择，
// 找更好的着法说明当时走了什么；揭示后的结果、标签与比较结论按题型说明，不说“棋谱中”。
import { BASE, bridge, check, launch, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const created = await bridge("create_study", {
  req: { name: "出题", kind: { type: "fen", fen: "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w", side: null }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
await bridge("apply_move", { req: { study_id: sid, parent: 0, mv: "e2c3", variation: false } });
for (let i = 0; i < 60; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 1) break;
  await sleep(250);
}

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
await page.getByRole("button", { name: "本局报告" }).click();
const report = page.getByTestId("report-dialog");
await report.getByTestId("report-generate").click();
const generated = await report.getByTestId("report-generated").innerText();
check(generated === "新增 3 道：找威胁 1、二选一 1、找更好的着法 1。", `写明新增几道：${generated}`);
await snap(page, "exercises-generated");
await page.keyboard.press("Escape");
await report.waitFor({ state: "hidden" });

await page.getByTestId("open-review").click();
const dialog = page.getByTestId("review-dialog");
const reviewCard = dialog.getByTestId("review-card");
const kinds = await dialog.getByTestId("card-kind").allInnerTexts();
check(["找威胁", "二选一", "找更好的着法"].every((k) => kinds.includes(k)), `复习库标出题型：${kinds.join("、")}`);

// 找威胁：点出将军的炮，提交
await dialog.locator(`[data-review="${sid}/0000000001/threat"]`).click();
await dialog.getByTestId("review-prompt").filter({ hasText: "点出受到威胁的己方棋子" }).waitFor();
check(await dialog.getByTestId("review-submit-marks").isDisabled(), "还没有点出格子时不能提交");
// 打开复习库时已经显示这张卡片，再点一次会重新读取；读取期间棋盘不接受点选，提示文字仍是上一次的。
// 直接派发的事件不等待可操作，先等按钮恢复可用（读取完成）再点格子
await dialog.locator('[data-testid="review-reveal"]:not([disabled])').waitFor();
await reviewCard.locator('[data-square="e7"]').first().dispatchEvent("mousedown", { button: 0 });
await dialog.getByTestId("review-submit-marks").filter({ hasText: "已点 1 处" }).waitFor();
check(await dialog.getByTestId("review-submit-marks").isEnabled(), "点出格子之后可以提交");
await snap(page, "threat-picked");
await dialog.getByTestId("review-submit-marks").click();
await dialog.getByTestId("review-result").waitFor();
check((await dialog.getByTestId("review-badge").innerText()) === "全部找到", "点出了将军的炮：全部找到");
check((await dialog.getByTestId("review-result").innerText()) === "点出了全部 1 处受到威胁的棋子。", "结果写明找到几处");
check((await dialog.getByTestId("review-origin").innerText()).includes("受到威胁"), "揭示后写明出处");

// 二选一：选了当时的着法
await dialog.locator(`[data-review="${sid}/0000000001/choice"]`).click();
await dialog.getByTestId("review-options").waitFor();
await dialog.getByTestId("review-option-e2c3").click();
await dialog.getByTestId("review-result").waitFor();
check((await dialog.getByTestId("review-badge").innerText()) === "没有选对", "选了当时的着法：没有选对");
const choice = await dialog.getByTestId("review-result").innerText();
check(choice === "这次选了 马五进七，与当时一样；引擎首选是 马五进三。", `结果按二选一说明：${choice}`);

// 找更好的着法：说明当时走了什么；直接看答案
await dialog.locator(`[data-review="${sid}/0000000001/better"]`).click();
await dialog.getByTestId("review-prompt").filter({ hasText: "当时走了 马五进七" }).waitFor();
check((await dialog.getByTestId("review-goal").count()) === 0, "找更好的着法总是局面决策，不选练习目标");
await dialog.getByTestId("review-reveal").click();
await dialog.getByTestId("review-result").waitFor();
const better = await dialog.getByTestId("review-result").innerText();
check(better.includes("引擎首选是 马五进三") && !better.includes("棋谱中"), `结果不说“棋谱中”：${better}`);
check((await dialog.getByTestId("review-played").innerText()) === "马五进七", "讲解与评价标明属于当时走的那一着");
await snap(page, "better-revealed");
// 再试一次，走出引擎首选：比较结论按题型说明，不说“与棋谱一致”
await dialog.getByRole("button", { name: "再试一次" }).click();
await dialog.getByTestId("review-prompt").waitFor();
for (const sq of ["e2", "g3"]) await reviewCard.locator(`[data-square="${sq}"]`).first().dispatchEvent("mousedown", { button: 0 });
const verdictEl = dialog.getByTestId("review-decision");
await verdictEl.filter({ hasNotText: "正在用同一次搜索比较" }).waitFor({ timeout: 60000 });
const verdict = (await verdictEl.innerText()).trim();
check(verdict === "与题目中的引擎首选一致", `找更好的着法走出引擎首选：${verdict}`);
check((await dialog.getByTestId("review-badge").innerText()) === "比较支持", "比较支持这个选择");
check(!(await dialog.getByTestId("review-self-check").innerText()).includes("棋谱"), "自我检查的提醒不说“棋谱”");

const events = (await bridge("learning_events")).filter((e) => e.study_id === sid);
// 找更好的着法作答了两次：直接看答案，再试一次走出引擎首选
const recorded = events.map((e) => `${e.kind}:${e.outcome}`).sort().join("、");
check(recorded === "better:independent、better:revealed、choice:missed、threat:independent", `学习事件记下题型与结果：${recorded}`);
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
