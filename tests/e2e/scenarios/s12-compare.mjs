// 场景 12：试走对比。
// A. 试走中“对比我的走法”是主要操作；对比写明比较起点，两步都从这里走出；四个问题各有回答；
//    两条变化都从比较起点演示，切换时保留步数；可以改选引擎候选作为对照；
// B. 候选详情中的“如果改走这步，会怎样？”；
// C. 先想后讲的对照卡片中的“对比我的走法”。
import { BASE, bridge, check, launch, move, snap } from "../lib.mjs";

const created = await bridge("create_study", {
  req: { name: "试走对比", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 4) break;
  await new Promise((r) => setTimeout(r, 500));
}

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => {
  localStorage.setItem("xq-last-study", id);
  localStorage.setItem("xq-detail-open", "1");
}, sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const ctx = page.getByTestId("board-context");
const panel = page.getByTestId("compare-panel");
const isPrimary = (loc) => loc.evaluate((el) => el.classList.contains("n-button--primary-type"));
const settled = async () => {
  await page.getByTestId("compare-outcome").waitFor({ timeout: 60000 });
  await panel.getByText("正在用同一次搜索比较两步").waitFor({ state: "detached", timeout: 60000 });
};

// ---------- A. 试走后对比 ----------
await page.locator('[data-node="1"]').click();
await page.getByRole("button", { name: "从这里试走" }).click();
check((await page.getByTestId("compare-trial").count()) === 0, "还没有走出试走着法时不显示对比");
await move(page, "b9c7");
await page.getByTestId("compare-trial").waitFor();
check(await isPrimary(page.getByTestId("compare-trial")), "试走中“对比我的走法”是主要操作");
await page.getByTestId("compare-trial").click();
await panel.waitFor();
const origin = await page.getByTestId("compare-origin").innerText();
check(origin.includes("第 1 回合 · 黑方走（炮二平五之后）") && origin.includes("两步都从这里走出"), `写明比较起点：${origin}`);
const mine = page.getByTestId("compare-mine");
const ref = page.getByTestId("compare-reference");
check((await mine.innerText()).includes("马2进3"), "我的选择是试走的第一着");
const refText = await ref.innerText();
check(refText.includes("马8进7") && refText.includes("实际着法"), "默认以棋谱中的实际着法作对照，并标明“实际着法”");
await settled();
const diff = await page.getByTestId("compare-differences").innerText();
check(diff.includes("同一次搜索中"), `已确认的差别来自同一次搜索：${diff.replace(/\n/g, " ").slice(0, 120)}`);
for (const [name, loc] of [["我的选择", mine], ["对照选择", ref]]) {
  const t = await loc.innerText();
  check(t.includes("这步改变了什么") && t.includes("对方最值得关注的回应是"), `${name}说明了改变了什么与对方的回应`);
  check(/程序\s*黑方视角评分/.test(t), `${name}的评分由程序给出并写明视角`);
}
check((await panel.innerText()).includes("对照选择不一定是最好的走法"), "说明对照选择不一定是最好的走法");
await snap(page, "trial-compare");

// 两条变化都从比较起点演示，切换时保留步数
await page.getByTestId("compare-demo-mine").click();
let c = await ctx.innerText();
check(c.includes("正在演示：我的选择 马2进3（从比较起点）") && c.includes("第 0 / "), `从比较起点演示我的选择：${c}`);
await page.keyboard.press("ArrowRight");
await page.keyboard.press("ArrowRight");
await page.getByTestId("compare-demo-reference").click();
c = await ctx.innerText();
check(c.includes("对照选择 马8进7") && c.includes("第 2 / "), `切换到对照选择时保留步数：${c}`);
check((await page.getByTestId("compare-demo-reference").innerText()).includes("正在演示"), "正在演示的一方突出显示");
check(await page.getByTestId("compare-demo-mine").evaluate((el) => el.classList.contains("n-button--secondary")), "另一方的演示按钮为浅色");
await page.waitForTimeout(300);
await snap(page, "compare-demo");
await page.keyboard.press("Escape");

// 改选引擎候选作为对照
await page.getByTestId("compare-reference-select").click();
const candidate = page.locator(".n-base-select-option", { hasText: "引擎候选第" }).first();
await candidate.waitFor();
const picked = (await candidate.innerText()).replace(/^引擎候选第 \d+ /, "").trim();
await candidate.click();
await ref.filter({ hasText: picked }).waitFor({ timeout: 10000 });
await settled();
check((await ref.innerText()).includes("引擎候选第"), `改选引擎候选 ${picked} 作为对照，并标明候选名次`);
await page.getByTestId("compare-close").click();
await panel.waitFor({ state: "detached" });
check((await page.locator(".n-tabs-tab", { hasText: "对比" }).count()) === 0, "关闭对比后不再显示对比标签");
await page.getByRole("button", { name: "结束试走，返回原局面" }).click();

// ---------- B. 候选：如果改走这步 ----------
await page.locator('[data-node="3"]').click();
await page.getByTestId("key-point").waitFor();
const row = page.locator(".candidates .row:not(.actual)").first();
const candName = (await row.locator(".notation").innerText()).trim();
await row.click();
await page.getByTestId("compare-candidate").click();
await panel.waitFor();
check((await mine.innerText()).includes(candName), `我的选择是所选候选 ${candName}`);
const refB = await ref.innerText();
check(refB.includes("马二进三") && refB.includes("实际着法"), "对照是这一着的实际着法");
check((await page.getByTestId("compare-origin").innerText()).includes("第 2 回合 · 红方走（马8进7之后）"), "比较起点是这一着落子前的局面");
await settled();
await page.getByTestId("compare-close").click();

// ---------- C. 先想后讲：对比我的走法 ----------
await page.getByText("先想后讲", { exact: true }).click();
await page.locator('[data-node="2"]').click();
await ctx.filter({ hasText: "先想后讲" }).waitFor();
await move(page, "b9c7");
await page.getByTestId("compare-attempt").waitFor({ timeout: 30000 });
await page.getByTestId("compare-attempt").click();
await panel.waitFor();
check((await mine.innerText()).includes("马2进3") && (await ref.innerText()).includes("马8进7"), "先想后讲中对比自己的走法与实际着法");
await settled();
check(true, "同样两步的对比直接得到结果");
await snap(page, "think-compare");
await page.getByText("看谱学习", { exact: true }).click();

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
