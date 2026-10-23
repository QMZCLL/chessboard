// 场景 9：学习工作台的操作逻辑（看谱学习、依据与演示、试走、先想后讲、候选、棋谱树、设置、笔记、布局）。
import { BASE, bridge, check, launch, move, snap } from "../lib.mjs";

const created = await bridge("create_study", {
  req: { name: "操作流程", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
// 等四着的讲解都有引擎分析
for (let i = 0; i < 120; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 4) break;
  await new Promise((r) => setTimeout(r, 500));
}

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => {
  localStorage.setItem("xq-last-study", id);
  localStorage.removeItem("xq-detail-open");
}, sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const ctx = page.getByTestId("board-context");
const selectedId = () => page.evaluate(() => document.querySelector(".move.active")?.getAttribute("data-node"));

// 1. 看谱学习：选择一着 → 要点 → 点击依据只高亮 → 演示变化 → 返回本步
await page.locator('[data-node="3"]').click();
await page.getByTestId("key-point").waitFor();
check((await page.getByTestId("move-name").innerText()).includes("马二进三"), "标题显示当前讲解的着法");
check((await ctx.innerText()).includes("棋谱局面 · 第 2 回合"), `棋盘上方说明当前局面：${await ctx.innerText()}`);
const basis = page.locator(".claims").first().locator(".claim").first();
await basis.click();
check(await basis.evaluate((el) => el.classList.contains("active")), "点击讲解段落后该段被选中");
check(!(await ctx.innerText()).includes("变化演示"), "点击段落不进入演示");
check((await page.locator(".xq-board circle.hl").count()) > 0, "依据在当前局面上时直接高亮棋子");
await snap(page, "wf-focus");

await page.getByRole("button", { name: "演示主要应对" }).click();
await ctx.filter({ hasText: "变化演示" }).waitFor();
check((await ctx.innerText()).includes("第 0 / "), `演示从起点开始：${await ctx.innerText()}`);
await page.keyboard.press("ArrowRight");
check((await ctx.innerText()).includes("第 1 / "), "方向键在演示中逐着前进");
check(await page.getByRole("button", { name: /保存已演示的 1 着/ }).isVisible(), "保存按钮写明保存范围");
await snap(page, "wf-demo");
await page.getByRole("button", { name: "返回本步" }).click();
check((await ctx.innerText()).includes("棋谱局面"), "返回本步后回到原来的局面");
check((await selectedId()) === "3", "返回后仍是原来的着法");

// 2. 落子前局面的依据：不画在落子后的棋盘上，提示起点
await page.getByText("详细分析", { exact: true }).click();
const comparison = page.locator(".claims", { hasText: "首选" }).first().locator(".claim").first();
await comparison.click();
const beforeNote = comparison.getByText("这条依据在落子前的局面上");
check(await beforeNote.isVisible(), "落子前局面的依据显示起点提示");
check((await page.locator(".xq-board line.arrow").count()) === 0, "不把落子前的箭头画在落子后的棋盘上");
await comparison.getByRole("button", { name: "在落子前局面查看" }).click();
await ctx.filter({ hasText: "落子前局面" }).waitFor();
check((await page.locator(".xq-board circle.hl").count()) > 0, "在落子前局面上标出依据");
await snap(page, "wf-before");
await page.keyboard.press("Escape");
check((await ctx.innerText()).includes("棋谱局面"), "Esc 退出查看");

// 3. 快捷键作用范围
await page.locator(".n-tabs-tab", { hasText: "笔记" }).click();
const noteBox = page.getByTestId("note-input").locator("textarea");
await noteBox.click();
await noteBox.type("中路要注意");
await page.keyboard.press("ArrowLeft");
check((await selectedId()) === "3", "在笔记中按方向键不会切换棋谱");
await page.locator(".xq-board").click({ position: { x: 5, y: 5 } });
await page.getByTestId("note-state").filter({ hasText: "已保存" }).waitFor({ timeout: 10000 });
check(true, "笔记离开输入框后显示“已保存”");
check(await page.locator('[data-node="3"] .note').isVisible(), "棋谱中显示笔记标记");
await page.getByRole("button", { name: "设置" }).click();
await page.locator(".n-drawer").waitFor();
await page.keyboard.press("ArrowLeft");
check((await selectedId()) === "3", "设置打开时方向键不切换棋谱");
await page.keyboard.press("Escape");
await page.locator(".n-drawer").waitFor({ state: "hidden" });
await page.keyboard.press("ArrowLeft");
check((await selectedId()) === "2", "回到棋盘后方向键正常切换");

// 4. 试走：从这里试走 → 走子 → 结束试走返回
await page.locator('[data-node="1"]').click();
await page.getByRole("button", { name: "从这里试走" }).click();
check((await ctx.innerText()).includes("试走分支"), "进入试走后说明走法会保存在分支中");
await move(page, "b9c7");
await page.waitForFunction(() => document.querySelector(".move.active")?.getAttribute("data-node") !== "1");
check((await ctx.innerText()).includes("试走分支"), "试走走子后仍处于试走状态");
await snap(page, "wf-trial");
await page.getByRole("button", { name: "结束试走，返回原局面" }).click();
check((await selectedId()) === "1", "结束试走后回到开始试走的局面");
check(await page.locator(".branch-head").first().isVisible(), "试走的着法保留为分支");

// 5. 分支折叠与删除确认
const head = page.locator(".branch-head").first();
await head.click();
check((await head.getAttribute("aria-expanded")) === "false", "分支可以折叠");
await head.click();
const branchMove = page.locator(".variation .move").first();
await branchMove.hover();
await branchMove.locator(".more").click();
await page.getByText(/删除此着及之后（共 \d+ 着）/).click();
const dialog = page.locator(".n-dialog");
await dialog.waitFor();
check((await dialog.innerText()).includes("无法撤销"), "删除前确认并说明影响范围");
const beforeKeys = await selectedId();
await page.keyboard.press("ArrowLeft");
check((await selectedId()) === beforeKeys, "确认框打开时方向键不切换棋谱");
await dialog.getByRole("button", { name: "取消" }).click();

// 6. 先想后讲
await page.getByText("先想后讲", { exact: true }).click();
await page.locator('[data-node="3"]').click();
await ctx.filter({ hasText: "先想后讲" }).waitFor();
check((await ctx.innerText()).includes("请为红方选择下一步"), "先想后讲说明为哪一方选择");
await page.getByRole("button", { name: "直接看讲解" }).click();
await page.getByTestId("key-point").waitFor();
await page.getByText("看谱学习", { exact: true }).click();

// 7. 候选：选中只显示详情，演示使用按钮
await page.getByRole("button", { name: "查看候选" }).click();
const row = page.locator(".candidates .row").first();
await row.click();
check(await page.locator(".candidates .detail").first().isVisible(), "选中候选显示详情");
check(!(await ctx.innerText()).includes("变化演示"), "选中候选不进入演示");
await row.getByRole("button", { name: "演示", exact: true }).click();
await ctx.filter({ hasText: "变化演示" }).waitFor();
check(true, "候选的演示按钮进入演示");
await page.getByRole("button", { name: "返回本步" }).click();

// 8. 起始局面
await page.locator('[data-node="0"]').click();
await page.getByTestId("start-intro").waitFor();
check(await page.getByTestId("analyze-position").isVisible(), "起始局面直接提供“分析这个局面”");

// 9. 设置：分组、保存状态、主题
await page.getByRole("button", { name: "设置" }).click();
const drawer = page.locator(".n-drawer");
await drawer.waitFor();
check((await page.getByTestId("settings-state").innerText()) === "所有修改已保存", "打开设置时没有未保存的修改");
for (const t of ["学习与显示", "AI 讲解", "分析引擎", "屏幕连接", "诊断"]) check(await drawer.locator(".n-tabs-tab", { hasText: t }).isVisible(), `设置包含“${t}”`);
await drawer.locator(".n-tabs-tab", { hasText: "分析引擎" }).click();
const multipv = drawer.locator(".n-form-item", { hasText: "候选数量" }).locator("input");
await multipv.fill("4");
await multipv.blur();
check((await page.getByTestId("settings-state").innerText()) === "有未保存的修改", "修改后提示有未保存的修改");
await multipv.fill("3");
await multipv.blur();
await drawer.locator(".n-tabs-tab", { hasText: "学习与显示" }).click();
await drawer.getByText("深色", { exact: true }).click();
check((await page.evaluate(() => document.documentElement.dataset.theme)) === "dark", "选择深色主题立即生效");
await page.waitForTimeout(600);
await snap(page, "wf-dark-settings");
await drawer.getByText("跟随系统", { exact: true }).click();
await page.keyboard.press("Escape");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();

// 10. 布局：中等宽度的棋谱浮层，窄窗口的标签切换
{
  const { browser, page, problems } = await launch({ width: 1100, height: 860 });
  await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
  await page.goto(BASE);
  await page.waitForSelector(".xq-board");
  check(!(await page.locator(".columns > aside").first().innerText()).includes("起始局面"), "中等宽度默认收起棋谱栏");
  await page.getByTestId("toggle-tree").click();
  await page.getByTestId("tree-overlay").waitFor();
  await page.getByTestId("tree-overlay").locator('[data-node="2"]').click();
  check((await page.getByTestId("move-name").innerText()).includes("马8进7"), "在棋谱浮层中选择着法");
  await snap(page, "wf-medium-tree");
  await page.keyboard.press("Escape");
  await page.getByTestId("tree-overlay").waitFor({ state: "hidden" });
  check(true, "Esc 先收起棋谱浮层");
  await page.getByTestId("toggle-tree").click();
  await page.locator(".backdrop").click({ position: { x: 600, y: 300 } });
  await page.getByTestId("tree-overlay").waitFor({ state: "hidden" });
  check(true, "点击浮层外收起棋谱");
  const box = await page.locator(".xq-board").boundingBox();
  check(box && box.width > 500, `中等宽度下棋盘宽 ${Math.round(box?.width ?? 0)}px`);
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}
{
  const { browser, page, problems } = await launch({ width: 760, height: 1000 });
  await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
  await page.goto(BASE);
  await page.waitForSelector(".xq-board");
  const box = await page.locator(".xq-board").boundingBox();
  check(box && box.width > 400, `窄窗口下棋盘宽 ${Math.round(box?.width ?? 0)}px`);
  await page.locator(".n-tabs-tab", { hasText: /^棋谱$/ }).click();
  await page.locator('.stack-panel [data-node="1"]').click();
  await page.locator(".n-tabs-tab", { hasText: /^讲解$/ }).click();
  check((await page.getByTestId("move-name").innerText()).includes("炮二平五"), "窄窗口下通过标签切换棋谱与讲解");
  const scrollX = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  check(!scrollX, "窄窗口没有横向滚动");
  await snap(page, "wf-narrow");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}
