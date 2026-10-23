// 场景 4：最小窗口尺寸与深色模式下的布局。
import { BASE, check, launch, snap, waitText } from "../lib.mjs";
const { browser, page, problems } = await launch({ width: 1024, height: 680, dark: true });
await page.goto(BASE);
await waitText(page, "建立学习局面");
await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
await page.locator(".saved-row", { hasText: "导入的棋谱" }).first().click();
// 1024 宽时棋谱栏默认收起，从顶栏打开浮层选择着法
await page.getByTestId("toggle-tree").waitFor();
check(true, "中等宽度提供“棋谱”按钮");
await page.getByTestId("toggle-tree").click();
await page.getByTestId("tree-overlay").locator(".line:not(.variation) > .move", { hasText: "马二进三" }).click();
await page.getByTestId("key-point").waitFor();
await page.getByTestId("toggle-tree").click();
await page.getByTestId("tree-overlay").waitFor({ state: "hidden" });
const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
check(overflow <= 0, `无横向滚动（溢出 ${overflow}px）`);
const board = await page.locator(".xq-board").boundingBox();
check(board && board.width > 300 && board.y + board.height <= 680, `棋盘完整显示（${Math.round(board.width)}×${Math.round(board.height)}）`);
await snap(page, "dark-1024");
await page.getByRole("button", { name: "新建 / 打开" }).click();
await page.locator(".n-tabs-tab", { hasText: "摆棋" }).click();
await page.waitForTimeout(300);
await snap(page, "dark-setup-1024");
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
