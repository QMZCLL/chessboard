// 场景 3：屏幕连接（模拟画面）：选择窗口 → 框选 → 黑方在下的起始局面确认 → 实时记录 → 漏帧补全
// → 用户在看历史节点时不被切走 → 无法解释的画面确认为缺口 → 停止。
import { BASE, bridge, check, launch, snap, waitText } from "../lib.mjs";

const START = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR";
const { browser, page, problems } = await launch();
await page.goto(BASE);
await waitText(page, "建立学习局面");
await page.keyboard.press("Escape");

// 模拟窗口里黑方在下
await bridge("dev_show_board", { board: START, orientation: "black_bottom" });
await page.getByRole("button", { name: "屏幕连接" }).click();
await page.locator(".window", { hasText: "模拟对弈窗口" }).click();
await waitText(page, "拖动方框对准棋盘");
check(await page.locator(".stage .dot").count() === 90, "框选区域显示 9×10 个交叉点参考点");
await snap(page, "calibrate");
await page.getByRole("button", { name: "开始连接" }).click();
await waitText(page, "请核对识别出的局面", 15000);
const orientation = await page.locator(".n-modal .n-radio-button--checked", { hasText: "黑方在下" }).count();
check(orientation === 1, "自动识别为黑方在下");
check(await page.locator(".n-modal .n-radio-button--checked", { hasText: "红走" }).count() === 1, "开局局面自动建议红走");
await snap(page, "confirm-start");
await page.getByRole("button", { name: "确认并开始记录" }).click();
await page.locator("[data-testid=board-context] .side.red").waitFor();
check(await page.locator(".top .n-tag", { hasText: "记录中" }).count() === 1, "顶部显示正在记录");

// 画面中红方走一步：炮二平五（标准坐标 h2e2），以黑方在下的画面给出
const apply = (fen, mv) => {
  const rows = fen.split("/").map((r) => r.replace(/\d/g, (d) => ".".repeat(Number(d))).split(""));
  const at = (sq) => [9 - Number(sq[1]), sq.charCodeAt(0) - 97];
  const [fr, fc] = at(mv.slice(0, 2));
  const [tr, tc] = at(mv.slice(2, 4));
  rows[tr][tc] = rows[fr][fc];
  rows[fr][fc] = ".";
  return rows.map((r) => r.join("").replace(/\.+/g, (m) => String(m.length))).join("/");
};
let screen = apply(START, "h2e2");
await bridge("dev_show_board", { board: screen, orientation: "black_bottom" });
await page.locator(".move", { hasText: "炮二平五" }).waitFor({ timeout: 15000 });
check(await page.locator(".move.active", { hasText: "炮二平五" }).count() === 1, "停留在最新局面时自动跟随新着法");
check(await page.locator(".move.live", { hasText: "炮二平五" }).count() === 1, "棋谱标记实时节点");

// 用户回到起始局面阅读时，新着法不应把视图切走
await page.locator(".root").click();
screen = apply(apply(screen, "h9g7"), "h0g2"); // 漏掉中间一帧：画面直接出现两步之后
await bridge("dev_show_board", { board: screen, orientation: "black_bottom" });
await page.locator(".move", { hasText: "马二进三" }).waitFor({ timeout: 15000 });
check(await page.locator(".move", { hasText: "马8进7" }).count() === 1, "漏帧时用两步合法走法补全（马8进7、马二进三）");
check(await page.locator(".root.active").count() === 1, "用户阅读起始局面时没有被切走");
await snap(page, "live-not-stolen");

// 无法解释的画面：弹出确认，校正后作为缺口继续
await bridge("dev_show_board", { board: "3k5/9/9/9/9/9/9/9/4R4/4K4", orientation: "black_bottom" });
await waitText(page, "无法用一到两步合法走法解释", 15000);
await page.locator(".n-modal .n-radio-button", { hasText: "黑走" }).first().click();
await snap(page, "confirm-gap");
await page.getByRole("button", { name: "按此局面继续" }).click();
// 只在棋谱中查找：屏幕连接对话框淡出期间，其中的提示框也带有 gap 样式类
const gap = page.locator(".tree .line .gap");
await gap.first().waitFor();
check((await gap.count()) === 1, "棋谱中出现带缺口标记的新起点");

// 停止连接
await page.getByRole("button", { name: "屏幕连接" }).click();
await page.getByRole("button", { name: "停止连接" }).click();
await page.locator(".window", { hasText: "模拟对弈窗口" }).waitFor();
await page.keyboard.press("Escape");
const status = await bridge("app_status");
check(status.capture.state === "idle", "停止后连接状态为 idle");
await snap(page, "after-capture");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
