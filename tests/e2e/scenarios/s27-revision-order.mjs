// 场景 27：棋谱树的版本不回退。别处（例如屏幕连接）连着加入两着，界面为每次 study_updated 各读取一次视图；
// 第一次读取较晚返回时，不用它较旧的棋谱树覆盖已经显示的较新的棋谱树；别处删除了正在看的这一着时，改选它的上一着。
import { BASE, bridge, check, hold, launch } from "../lib.mjs";

const created = await bridge("create_study", {
  req: { name: "版本乱序", kind: { type: "record", text: "1. 炮二平五" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const mainMoves = page.locator(".line:not(.variation) > .move");
await mainMoves.first().waitFor();

// 只拦住第一次读取的响应
let reads = 0;
const held = await hold(page, "study_view", { match: () => reads++ === 0 });
const second = await bridge("apply_move", { req: { study_id: sid, parent: 1, mv: "h9g7", variation: false } });
await held.arrived;
await bridge("apply_move", { req: { study_id: sid, parent: second.node_id, mv: "h0g2", variation: false } });
await mainMoves.nth(2).waitFor();
check((await mainMoves.count()) === 3, "第二次读取先到：显示加入的两着");
// 之后的读取都先留住：较晚到达的旧视图如果让棋谱树回退，不能靠再读取一次悄悄恢复
const parked = [];
const park = (route) => void parked.push(route);
await page.route("**/__bridge/invoke/study_view", park);
await held.release();
const count = await mainMoves.count();
check(count === 3, `第一次读取较晚到达：棋谱树不回退（主线 ${count} 着）`);
check(parked.length === 0, `完全较旧的视图被忽略，不需要再读取（再读取 ${parked.length} 次）`);
await page.unroute("**/__bridge/invoke/study_view", park);
for (const r of parked) await r.continue();
// 别处删除了正在看的这一着：改选还在的最近一着（它的上一着）
const last = await page.locator(".line:not(.variation) > .move").nth(2).getAttribute("data-node");
await page.locator(`[data-node="${last}"]`).click();
await page.locator(`[data-node="${last}"][aria-current="step"]`).waitFor();
await bridge("delete_node", { req: { study_id: sid, node_id: Number(last) } });
await page.locator(`[data-node="${second.node_id}"][aria-current="step"]`).waitFor();
check((await mainMoves.count()) === 2, "删除之后选中上一着，主线两着");
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
