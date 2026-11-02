// 场景 21：分析稳定性（模拟引擎）。快速分析最后一个深度首选才换过：讲解标为“排名仍在变化”，写明依据，
// 不说“是首选”，不按分差评价；随后在后台补充一次深入分析，期间说明原因，结果稳定后标为“高稳定”。
import { BASE, check, launch, snap, waitText } from "../lib.mjs";

const { browser, page, problems } = await launch();
const gos = [];
page.on("request", (r) => {
  if (r.url().includes("/__bridge/invoke/explain_move")) gos.push(JSON.parse(r.postData() ?? "{}"));
});
await page.goto(BASE);
await waitText(page, "建立学习局面");
await page.locator(".n-tabs-tab", { hasText: "棋谱" }).click();
await page.getByPlaceholder("粘贴棋谱").fill("1. 马二进三");
await page.getByRole("button", { name: "导入棋谱" }).click();
await page.locator(".move", { hasText: "马二进三" }).click();
const panel = page.locator(".panel").first();
const tag = panel.getByTestId("stability-tag");
await tag.filter({ hasText: "排名仍在变化" }).waitFor({ timeout: 30000 });
check(true, "标题旁标出“排名仍在变化”");
check((await panel.locator(".head-tags").innerText()).includes("与首选接近"), "差距很小的结论照常给出");
await page.getByText("详细分析", { exact: true }).click();
const note = await panel.getByTestId("stability-note").innerText();
check(note.includes("最后一个深度（4）首选才由马二进三换成炮二平五") && note.includes("不评价哪一着更好"), `写明依据：${note}`);
const comparison = await panel.innerText();
check(comparison.includes("当前深度的首选是炮二平五") && !comparison.includes("在当前分析中"), "不称“引擎首选”，说明首选仍在变化");
await panel.getByTestId("explain-pending").filter({ hasText: "正在补充深入分析" }).waitFor({ timeout: 10000 });
check(true, "后台补充深入分析时说明原因");
await snap(page, "stability-shifting");

// 补充的深入分析完成：结论稳定
await tag.filter({ hasText: "高稳定" }).waitFor({ timeout: 30000 });
check((await panel.locator(".head-tags").innerText()).includes("深入分析完整"), "深入分析的结果替换快速分析");
check(!(await panel.innerText()).includes("当前深度的首选是"), "稳定后照常说明首选");
check(gos.every((b) => b.req.budget === "quick"), "补充深入分析由后台安排，界面没有另外请求");
await snap(page, "stability-settled");

// 设置中可以关闭
await page.getByRole("button", { name: "设置" }).click();
const toggle = page.getByTestId("settings-deepen");
await toggle.waitFor();
check((await toggle.getAttribute("aria-checked")) === "true", "设置中“补充深入分析”默认开启");
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
