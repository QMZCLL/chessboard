// 场景 25：会话视图较晚到达（模拟引擎，每次搜索 3 秒）。所有视图都按棋谱树与讲解部分分别合并，较旧的部分不覆盖较新的：
// 1. 走子的响应晚于基本讲解事件到达，接着重新读取的视图又在讲解完成之后才到达：都不覆盖已经显示的统计；
// 2. 讲解已经完成后，较早的走子响应才到达：重新读取被拦住期间界面也不回退（没有一闪而过的“进行中”），
//    同一时间只有一个重新读取。
// 不用固定的等待：界面处理完较晚的响应后才会发出重新读取，以这个请求到达作为检查的时机。
import { BASE, bridge, check, hold, launch, move, snap } from "../lib.mjs";

const created = await bridge("create_study", {
  req: { name: "视图刷新", kind: { type: "fen", fen: "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w", side: null }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const stats = page.locator(".stats").first();
const mainMoves = page.locator(".line:not(.variation) > .move");
let viewReads = 0;
page.on("request", (r) => r.url().includes("/__bridge/invoke/study_view") && (viewReads += 1));

// 1. 走子的响应留住，先让基本讲解事件到达
const moveHeld = await hold(page, "apply_move");
await move(page, "e2c3");
await moveHeld.arrived;
await stats.filter({ hasText: "进行中 1" }).waitFor();
// 放行走子响应：它的讲解部分早于已收到的事件，只采用棋谱树并重新读取；这次读取留到讲解完成之后再放行
const viewHeld = await hold(page, "study_view");
await moveHeld.release();
await viewHeld.arrived;
check((await mainMoves.count()) === 1 && (await stats.innerText()).includes("进行中 1"), "较晚的走子响应只带来棋谱树，讲解统计不回退");
await stats.filter({ hasText: "讲解 1/1" }).waitFor({ timeout: 15000 });
await viewHeld.release();
const first = await stats.innerText();
check(first.includes("讲解 1/1") && !first.includes("进行中"), `较晚到达的视图不覆盖已经显示的统计：${first}`);

// 2. 讲解完成之后，较早的走子响应才到达；重新读取一直拦住
const moveHeld2 = await hold(page, "apply_move");
await move(page, "d9d8");
await moveHeld2.arrived;
await stats.filter({ hasText: "讲解 2/2" }).waitFor({ timeout: 15000 });
const before = viewReads;
const viewHeld2 = await hold(page, "study_view");
await moveHeld2.release();
await viewHeld2.arrived;
const during = await stats.innerText();
check(during.includes("讲解 2/2") && !during.includes("进行中"), `重新读取还被拦住时界面不回退：${during}`);
check((await mainMoves.count()) === 2, "棋谱中已经有这一着");
await viewHeld2.release();
const after = await stats.innerText();
check(after.includes("讲解 2/2") && !after.includes("进行中"), `重新读取到达后仍是：${after}`);
check(viewReads - before === 1, `同一时间只有一个重新读取（读取 ${viewReads - before} 次）`);
await snap(page, "stats-after-late-view");
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
