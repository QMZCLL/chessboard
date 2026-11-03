// 场景 25：会话视图较晚到达（模拟引擎，搜索 3 秒）。走子命令的响应晚于基本讲解事件到达时，界面重新获取会话视图；
// 这次获取在引擎完成之前读取、在讲解完成事件之后才到达时，不用它覆盖已经显示的讲解统计，而是再获取一次。
import { BASE, bridge, check, hold, launch, move, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const created = await bridge("create_study", {
  req: { name: "视图刷新", kind: { type: "fen", fen: "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w", side: null }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const stats = page.locator(".stats").first();

// 走子命令的响应留住，先让基本讲解事件到达
const moveHeld = await hold(page, "apply_move");
await move(page, "e2c3");
await moveHeld.arrived;
await stats.filter({ hasText: "进行中 1" }).waitFor();
// 放行走子响应：它早于已收到的讲解事件，界面重新获取视图；这次获取在引擎完成之前读取，留到讲解完成之后再放行
const viewHeld = await hold(page, "study_view");
await moveHeld.release();
await viewHeld.arrived;
await stats.filter({ hasText: "讲解 1/1" }).waitFor({ timeout: 15000 });
check(!(await stats.innerText()).includes("进行中"), "讲解完成事件更新了统计");
await viewHeld.release();
await sleep(1000);
const text = await stats.innerText();
check(text.includes("讲解 1/1") && !text.includes("进行中"), `较晚到达的视图不覆盖已经显示的统计：${text}`);
await snap(page, "stats-after-late-view");
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
