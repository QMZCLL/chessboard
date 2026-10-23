// 场景 8：评分边界值（模拟引擎）。候选列表与讲解都不能把“将杀分上界”显示成几步杀，
// 普通分的边界值注明“≥/≤”。
import { BASE, check, launch, snap, waitText } from "../lib.mjs";

const { browser, page, problems } = await launch();
await page.goto(BASE);
await waitText(page, "建立学习局面");
await page.locator(".n-tabs-tab", { hasText: "棋谱" }).click();
await page.getByPlaceholder("粘贴棋谱").fill("1. 马二进三");
await page.getByRole("button", { name: "导入棋谱" }).click();
await page.locator(".move", { hasText: "马二进三" }).click();
await page.locator(".panel .n-tag", { hasText: /引擎分析完整|分析不完整/ }).waitFor({ timeout: 30000 });
await page.getByText("详细分析", { exact: true }).click();
const scores = await page.getByTestId("candidate-score").allInnerTexts();
console.log(`  候选评分：${scores.join(" / ")}`);
check(scores.some((s) => s.includes("将杀边界，待确认")), "将杀分上界显示为“将杀边界，待确认”");
check(scores.some((s) => s.includes("≥ +20")), "普通分下界显示为“≥ +20”");
check(!scores.some((s) => s.includes("步杀")), "候选列表没有“几步杀”");
const panel = await page.locator(".panel").first().innerText();
check(!panel.includes("3 步杀") && !panel.includes("3 步内杀棋"), "讲解中没有确定的杀棋步数");
check(panel.includes("将杀边界值"), "讲解说明出现将杀边界值");
await snap(page, "bounds");
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
