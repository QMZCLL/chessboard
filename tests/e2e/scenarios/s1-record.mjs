// 场景 1：导入棋谱 → 逐着讲解 → 证据高亮 → 候选演示 → 加入分支 → 非法走法提示。
import { BASE, bridge, check, launch, move, snap, waitText } from "../lib.mjs";

const { browser, page, problems } = await launch();
await page.goto(BASE);
await waitText(page, "建立学习局面");
await snap(page, "start-dialog");

await page.locator(".n-tabs-tab", { hasText: "棋谱" }).click();
await page.getByPlaceholder("粘贴棋谱").fill("1. 炮二平五 马8进7 2. 马二进三 车9平8");
await page.getByRole("button", { name: "导入棋谱" }).click();
await page.locator(".move", { hasText: "车9平8" }).waitFor();
check(true, "棋谱导入后出现 4 着");

// 选中“马二进三”，等待详细讲解
await page.locator(".move", { hasText: "马二进三" }).click();
await page.locator(".panel .n-tag", { hasText: "引擎分析完整" }).waitFor({ timeout: 60000 });
const summary = await page.getByTestId("key-point").innerText();
console.log(`  要点：${summary}`);
check((await page.getByTestId("move-name").innerText()) === "马二进三", "讲解标题是当前着法");
check(summary.length > 0 && !summary.startsWith("马二进三："), "一句话要点不重复着法名");
check(await page.locator(".panel h3", { hasText: "为什么这样走" }).count() === 1, "讲解包含“为什么这样走”");
check(await page.locator(".panel .lesson-title", { hasText: "学习点" }).count() === 1, "讲解包含“学习点”");
await page.getByText("详细分析", { exact: true }).click();
for (const h of ["当前需求", "候选比较", "主要应对"]) {
  check(await page.locator(".panel h4", { hasText: h }).count() === 1, `详细分析包含“${h}”`);
}
const candidates = await page.locator(".panel .candidates .row").count();
check(candidates >= 2, `候选比较列出 ${candidates} 个候选`);
await snap(page, "move-explanation");

// 点击一条依据：棋盘出现高亮
await page.locator(".panel .claims .claim").first().click();
const hl = await page.locator(".xq-board circle.hl").count();
check(hl > 0, `点击依据后棋盘高亮 ${hl} 处`);
await snap(page, "evidence-highlight");

// 演示一个不是实际着法的候选
await page.locator(".panel .candidates .row:not(.actual)").first().getByRole("button", { name: "演示", exact: true }).click();
await page.getByTestId("preview-back").waitFor();
const demoCtx = await page.getByTestId("board-context").innerText();
check(demoCtx.includes("正在演示：马二进三之前的候选"), `棋盘上方写明演示的对象：${demoCtx}`);
check((await page.getByTestId("preview-back").innerText()).includes("返回马二进三后的局面"), "返回按钮写明回到哪个局面");
await page.getByRole("button", { name: "下一步" }).click();
await snap(page, "preview");
await page.getByRole("button", { name: /保存已演示的 1 着/ }).click();
await page.locator(".line.variation .move").first().waitFor();
check(await page.locator(".line.variation").count() >= 1, "变化已作为分支加入棋谱");
await snap(page, "branch-added");

// 回到最后一着，试一步非法走法：红马蹩腿
await page.locator(".line:not(.variation) > .move", { hasText: "车9平8" }).click();
await move(page, "b0d1");
await waitText(page, "不能这样走");
const illegal = await page.locator(".illegal").innerText();
console.log(`  ${illegal}`);
check(illegal.includes("蹩马腿"), "非法走法给出具体原因（蹩马腿）");
await snap(page, "illegal-move");

// 合法走子：车一平二，生成新节点与讲解
await move(page, "i0h0");
await page.locator(".move.active", { hasText: "车一平二" }).waitFor();
check(true, "走子后选中新节点“车一平二”");
await page.getByTestId("key-point").waitFor();
await snap(page, "new-move");

const status = await bridge("app_status");
check(status.engine.state === "ready", `引擎状态 ${status.engine.state}`);
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
