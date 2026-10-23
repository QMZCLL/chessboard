// 场景 6：不完整分析的重试（模拟引擎）。第一次搜索不完整：不能给出“严重失误”等结论；
// 普通查看不重新搜索；点“重试”后请求带 retry，引擎收到第二次搜索，讲解变为详细。
import { BASE, check, launch, snap, waitText } from "../lib.mjs";

const { browser, page, problems } = await launch();
const explainBodies = [];
page.on("request", (r) => {
  if (r.url().includes("/__bridge/invoke/explain_move")) explainBodies.push(JSON.parse(r.postData() ?? "{}"));
});
await page.goto(BASE);
await waitText(page, "建立学习局面");
await page.locator(".n-tabs-tab", { hasText: "棋谱" }).click();
await page.getByPlaceholder("粘贴棋谱").fill("1. 马二进三");
await page.getByRole("button", { name: "导入棋谱" }).click();
await page.locator(".move", { hasText: "马二进三" }).click();
await page.locator(".panel .n-tag", { hasText: "分析不完整" }).waitFor({ timeout: 30000 });
await page.getByText("详细分析", { exact: true }).click();
const panel = page.locator(".panel").first();
const partialText = await panel.innerText();
check(!partialText.includes("严重失误"), "不完整的候选没有判为严重失误");
check(partialText.includes("比较尚未完成"), "评价标签显示“比较尚未完成”");
check(partialText.includes("暂不比较"), "候选比较说明暂不比较");
check(explainBodies.length >= 1 && explainBodies.every((b) => b.req.retry === false), "普通查看的请求不带重试");
await snap(page, "retry-partial");

await page.getByRole("button", { name: "重试", exact: true }).click();
await page.locator(".panel .n-tag", { hasText: "引擎分析完整" }).waitFor({ timeout: 30000 });
const retry = explainBodies.at(-1);
check(retry?.req.retry === true && retry.req.budget === "quick", `重试请求带 retry 与原预算：${JSON.stringify(retry?.req)}`);
const detailedText = await panel.innerText();
check(detailedText.includes("与首选接近"), "重试后按完整的第二次搜索给出评价");
check(!detailedText.includes("比较尚未完成"), "不再显示未完成提示");
await snap(page, "retry-detailed");
console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
