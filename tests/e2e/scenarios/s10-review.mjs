// 场景 10：审阅意见的回归。
// A. 笔记保存过程中跨越布局断点：新面板显示新内容，再次失焦不会把旧值写回；
// B. 试走中前后移动保留试走起点，回到起点仍在试走中，沿棋谱前进则离开；
// C. 行内按钮的键盘操作：焦点在“演示”“更多操作”按钮上时，Enter 与空格触发按钮本身。
import { BASE, bridge, check, launch, move } from "../lib.mjs";

const created = await bridge("create_study", {
  req: { name: "审阅回归", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 4) break;
  await new Promise((r) => setTimeout(r, 500));
}
const ctxText = (page) => page.getByTestId("board-context").innerText();
const selectedId = (page) => page.evaluate(() => document.querySelector(".move.active, .root.active")?.getAttribute("data-node"));

const only = process.env.S10 ?? "ABC";
// ---------- A. 笔记与布局断点 ----------
if (only.includes("A")) {
  const { browser, page, problems } = await launch({ width: 1100, height: 860 });
  await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
  const saves = [];
  await page.route("**/__bridge/invoke/set_note", async (route) => {
    saves.push(JSON.parse(route.request().postData() ?? "{}"));
    // 第一次保存延迟返回，期间跨越断点
    if (saves.length === 1) await new Promise((r) => setTimeout(r, 2500));
    await route.continue();
  });
  await page.goto(BASE);
  await page.waitForSelector(".xq-board");
  // 中等宽度下棋谱栏收起：打开会话时选中的是主线最后一着
  const initial = await bridge("study_view", { id: sid });
  const nodeId = Object.values(initial.study.nodes).reduce((a, n) => (n.ply > a.ply ? n : a)).id;
  await page.locator(".n-tabs-tab", { hasText: "笔记" }).click();
  const box = page.getByTestId("note-input").locator("textarea");
  await box.click();
  await box.fill("跨断点保存的新笔记");
  await page.locator(".xq-board").click({ position: { x: 5, y: 5 } });
  await page.getByTestId("note-state").filter({ hasText: "保存中" }).waitFor();
  // 保存尚未返回时跨过 960px 断点，讲解面板重建
  await page.setViewportSize({ width: 900, height: 980 });
  await page.locator(".stack-panel").waitFor();
  await page.locator(".stack-panel .n-tabs-tab", { hasText: "笔记" }).click();
  const box2 = page.getByTestId("note-input").locator("textarea");
  check((await box2.inputValue()) === "跨断点保存的新笔记", "新面板显示保存中的新笔记，而不是旧值");
  await page.getByTestId("note-state").filter({ hasText: "已保存" }).waitFor({ timeout: 8000 });
  check(true, "保存完成后新面板显示“已保存”");
  // 再次进入并离开输入框：没有修改，不应再写入
  await box2.click();
  await page.locator(".xq-board").click({ position: { x: 5, y: 5 } });
  await page.waitForTimeout(800);
  check(saves.length === 1, `没有修改时不再保存（共 ${saves.length} 次写入）`);
  const view = await bridge("study_view", { id: sid });
  check(view.study.nodes[String(nodeId)].note === "跨断点保存的新笔记", "后台保存的是新笔记");
  // 未保存的修改跨断点保留
  await box2.click();
  await box2.fill("修改后尚未保存");
  await page.setViewportSize({ width: 1100, height: 860 });
  await page.locator(".n-tabs-tab", { hasText: "笔记" }).click();
  const box3 = page.getByTestId("note-input").locator("textarea");
  check((await box3.inputValue()) === "修改后尚未保存", "未保存的修改在布局切换后仍在");
  await page.waitForTimeout(800);
  const after = await bridge("study_view", { id: sid });
  check(after.study.nodes[String(nodeId)].note === "修改后尚未保存", "面板移除时保存了正在编辑的内容");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}

// ---------- B. 试走中的前后移动 ----------
if (only.includes("B")) {
  const { browser, page, problems } = await launch({ width: 1440, height: 900 });
  await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
  await page.goto(BASE);
  await page.waitForSelector(".xq-board");
  await page.locator('[data-node="1"]').click();
  await page.getByRole("button", { name: "从这里试走" }).click();
  await move(page, "b9c7");
  await page.waitForFunction(() => document.querySelector(".move.active")?.getAttribute("data-node") !== "1");
  const first = await selectedId(page);
  await move(page, "h0g2");
  await page.waitForFunction((id) => document.querySelector(".move.active")?.getAttribute("data-node") !== id, first);
  check((await ctxText(page)).includes("试走分支"), "试走走出两着");
  await page.getByRole("button", { name: "上一步" }).click();
  await page.waitForFunction((id) => document.querySelector(".move.active")?.getAttribute("data-node") === id, first);
  check((await ctxText(page)).includes("试走分支"), "试走中后退一着仍在试走中");
  check(await page.getByRole("button", { name: "结束试走，返回原局面" }).isVisible(), "仍可结束试走");
  await page.keyboard.press("ArrowLeft");
  await page.waitForFunction(() => document.querySelector(".move.active")?.getAttribute("data-node") === "1");
  check((await ctxText(page)).includes("试走分支"), "回到试走起点仍在试走中，可以换一种走法");
  await page.keyboard.press("ArrowRight");
  await page.waitForFunction(() => document.querySelector(".move.active")?.getAttribute("data-node") !== "1");
  check(!(await ctxText(page)).includes("试走分支"), "从起点沿棋谱原有着法前进，离开试走");
  // 重新试走：走一着、后退、再结束，回到最初的起点
  await page.locator('[data-node="1"]').click();
  await page.getByRole("button", { name: "从这里试走" }).click();
  await move(page, "b9c7");
  await page.waitForFunction(() => document.querySelector(".move.active")?.getAttribute("data-node") !== "1");
  await move(page, "h0g2");
  await page.waitForTimeout(400);
  await page.getByRole("button", { name: "上一步" }).click();
  await page.waitForTimeout(300);
  await page.getByRole("button", { name: "结束试走，返回原局面" }).click();
  await page.waitForFunction(() => document.querySelector(".move.active")?.getAttribute("data-node") === "1");
  check(true, "试走两着、后退一着后结束试走，回到最初记录的起点");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}

// ---------- C. 行内按钮的键盘操作 ----------
if (only.includes("C")) {
  const { browser, page, problems } = await launch({ width: 1440, height: 900 });
  await page.addInitScript((id) => {
    localStorage.setItem("xq-last-study", id);
    localStorage.setItem("xq-detail-open", "1");
  }, sid);
  await page.goto(BASE);
  await page.waitForSelector(".xq-board");
  await page.locator('[data-node="3"]').click();
  await page.getByTestId("key-point").waitFor();
  const focusedIs = (sel) => page.evaluate((s) => document.activeElement?.matches(s) ?? false, sel);

  // 讲解段落内的“演示变化”
  const claim = page.locator(".claims .claim", { has: page.locator(".demo") }).first();
  for (const key of ["Enter", "Space"]) {
    await claim.focus();
    await page.keyboard.press("Tab");
    check(await focusedIs(".claim .demo"), `Tab 把焦点移到段落内的“演示变化”（${key}）`);
    await page.keyboard.press(key === "Space" ? " " : "Enter");
    await page.getByTestId("board-context").filter({ hasText: "变化演示" }).waitFor({ timeout: 3000 });
    check(true, `焦点在“演示变化”上按 ${key} 进入演示`);
    await page.keyboard.press("Escape");
    await page.getByTestId("board-context").filter({ hasNotText: "变化演示" }).waitFor();
  }
  // 段落本身获得焦点时，Enter 只选中段落
  await claim.focus();
  await page.keyboard.press("Enter");
  check(await claim.evaluate((el) => el.classList.contains("active")), "焦点在段落上按 Enter 选中段落");
  check(!(await ctxText(page)).includes("变化演示"), "选中段落不进入演示");

  // 候选行内的“演示”
  const row = page.locator(".candidates .row").first();
  for (const key of ["Enter", "Space"]) {
    await row.focus();
    await page.keyboard.press("Tab");
    check(await focusedIs(".row .demo"), `Tab 把焦点移到候选行内的“演示”（${key}）`);
    await page.keyboard.press(key === "Space" ? " " : "Enter");
    await page.getByTestId("board-context").filter({ hasText: "变化演示" }).waitFor({ timeout: 3000 });
    check(true, `焦点在候选“演示”上按 ${key} 进入演示`);
    await page.keyboard.press("Escape");
    await page.getByTestId("board-context").filter({ hasNotText: "变化演示" }).waitFor();
  }

  // 棋谱着法行内的“更多操作”
  const active = page.locator(".move.active");
  await active.focus();
  await page.keyboard.press("Tab");
  check(await focusedIs(".move .more"), "Tab 把焦点移到着法行内的“更多操作”");
  await page.keyboard.press("Enter");
  await page.locator(".n-dropdown-menu").waitFor({ timeout: 3000 });
  check(true, "焦点在“更多操作”上按 Enter 打开菜单");
  await page.keyboard.press("Escape");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  } else {
    console.log("  ✓ 控制台无错误");
  }
  await browser.close();
}
