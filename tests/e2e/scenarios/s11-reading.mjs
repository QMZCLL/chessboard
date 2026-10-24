// 场景 11：讲解阅读与操作体验。
// A. 演示时写明演示的对象与返回的局面，主要操作随状态变化（演示中是“下一步”，到最后一着是返回）；
// B. 新结果到达时不打断：深入分析完成不退出演示、不收起详细分析；正在写的笔记不受影响；
// C. AI 讲解到达时保持阅读位置；有效 AI 讲解只有一套摘要与学习点；
// D. 分析不完整时“补充分析”是主要操作；
// E. 分隔条可以用键盘调整、显示宽度、恢复默认；
// F. 窄窗口下棋盘操作可见、可点击；浅色与深色主题下关键文字的对比度。
import { BASE, bridge, check, launch, snap } from "../lib.mjs";

const LLM = process.env.MOCK_LLM ?? "http://127.0.0.1:1440";
const created = await bridge("create_study", {
  req: { name: "阅读体验", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 4) break;
  await new Promise((r) => setTimeout(r, 500));
}
// AI 讲解使用模拟模型服务，只在点击时生成
await fetch(`${LLM}/__reset`, { method: "POST" });
const status = await bridge("app_status");
await bridge("update_config", {
  config: {
    ...status.config,
    coach: {
      ...status.config.coach,
      enabled: true,
      provider: "deepseek",
      base_url: LLM,
      model: "mock-model",
      auto_explain_scope: "off",
      audience: "beginner",
      detail: "normal",
    },
  },
});
await bridge("set_coach_credential", { req: { key: "sk-e2e-reading" } });

const open = async (opts, init = {}) => {
  const env = await launch(opts);
  await env.page.addInitScript(
    ([id, theme, detail]) => {
      localStorage.setItem("xq-last-study", id);
      localStorage.setItem("xq-theme", theme);
      localStorage.setItem("xq-detail-open", detail ? "1" : "0");
    },
    [sid, init.theme ?? "light", init.detail ?? false],
  );
  await env.page.goto(BASE);
  await env.page.waitForSelector(".xq-board");
  return env;
};
const isPrimary = (loc) => loc.evaluate((el) => el.classList.contains("n-button--primary-type"));
/** 一排按钮占几行 */
const rowsOf = (loc) =>
  loc.evaluate((el) => new Set([...el.querySelectorAll(".n-button")].map((b) => Math.round(b.getBoundingClientRect().top))).size);
const ctxText = (page) => page.getByTestId("board-context").innerText();

/** 前景色与合成后的背景色之间的对比度（WCAG 公式）。 */
function contrastOf(loc) {
  return loc.evaluate((el) => {
    const parse = (s) => {
      let m = s.match(/^rgba?\(([^)]+)\)/);
      if (m) {
        const p = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
        return { c: p.slice(0, 3), a: p.length > 3 ? p[3] : 1 };
      }
      m = s.match(/^color\(srgb ([^)]+)\)/);
      if (m) {
        const p = m[1].split(/[\s/]+/).filter(Boolean).map(Number);
        return { c: p.slice(0, 3).map((v) => v * 255), a: p.length > 3 ? p[3] : 1 };
      }
      return { c: [0, 0, 0], a: 0 };
    };
    const over = (top, under) => top.c.map((v, i) => v * top.a + under[i] * (1 - top.a));
    const layers = [];
    for (let n = el; n; n = n.parentElement) layers.push(parse(getComputedStyle(n).backgroundColor));
    let bg = [255, 255, 255];
    for (const l of layers.reverse()) bg = over(l, bg);
    const fg = over(parse(getComputedStyle(el).color), bg);
    const lum = (c) => {
      const [r, g, b] = c.map((v) => {
        v /= 255;
        return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const [hi, lo] = [lum(fg), lum(bg)].sort((a, b) => b - a);
    return (hi + 0.05) / (lo + 0.05);
  });
}

// ---------- A. 演示的对象与主要操作 ----------
{
  const { browser, page, problems } = await open({ width: 1440, height: 900 });
  await page.locator('[data-node="3"]').click();
  await page.getByTestId("key-point").waitFor();
  check(!(await isPrimary(page.getByRole("button", { name: "新建 / 打开" }))), "全局的“新建 / 打开”不占用最强的视觉强调");
  check(await isPrimary(page.getByTestId("demo-reply")), "阅读讲解时，“演示主要应对”是主要操作");
  await page.getByTestId("demo-reply").click();
  await page.getByTestId("preview-back").waitFor();
  const ctx = await ctxText(page);
  check(ctx.includes("正在演示：马二进三的主要应对") && ctx.includes("第 0 / "), `棋盘上方写明演示的对象与进度：${ctx}`);
  const subject = await page.getByTestId("explain-subject").innerText();
  check(subject.includes("本页讲解：马二进三"), `讲解面板说明本页讲解的是哪一着：${subject.replace(/\n/g, " ")}`);
  check((await page.getByTestId("preview-back").innerText()).includes("返回马二进三后的局面"), "返回按钮写明回到哪个局面");
  check(await isPrimary(page.getByTestId("preview-next")), "演示中“下一步”是主要操作");
  check(!(await isPrimary(page.getByTestId("preview-back"))), "演示未结束时，返回是次要操作");
  check((await rowsOf(page.getByTestId("board-controls"))) === 1, "演示工具栏保持一行");
  check((await rowsOf(page.locator(".key-actions"))) === 1, "讲解中的关键操作保持一行");
  await snap(page, "demo-subject");
  await page.getByRole("button", { name: "终点" }).click();
  check(await isPrimary(page.getByTestId("preview-back")), "演示到最后一着后，返回成为主要操作");
  check(await page.getByTestId("preview-next").isDisabled(), "最后一着时“下一步”不可用");
  await page.locator(".subject-bar").getByRole("button").click();
  check(!(await ctxText(page)).includes("正在演示"), "讲解面板中的返回按钮同样结束演示");
  check(await page.getByTestId("explain-subject").count() === 0, "棋盘回到讲解的局面后不再显示说明");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}

// ---------- B. 新结果到达时不打断正在进行的操作 ----------
{
  const { browser, page, problems } = await open({ width: 1440, height: 900 }, { detail: true });
  await page.locator('[data-node="3"]').click();
  await page.getByTestId("key-point").waitFor();
  check(await page.locator(".detail .n-collapse-item--active").isVisible(), "详细分析处于展开状态");
  await page.getByRole("button", { name: "深入分析", exact: true }).click();
  await page.getByTestId("demo-reply").click();
  await page.getByTestId("preview-next").click();
  const before = await ctxText(page);
  await page.locator(".panel .n-tag", { hasText: "深入分析完整" }).waitFor({ timeout: 60000 });
  const after = await ctxText(page);
  check(after === before, `深入分析结果到达后仍停在演示的同一着（${after}）`);
  check(await page.locator(".detail .n-collapse-item--active").isVisible(), "结果到达后详细分析仍然展开");
  await page.getByTestId("preview-back").click();

  // 正在写笔记时，另一着的深入分析结果到达
  await page.locator('[data-node="2"]').click();
  await page.getByTestId("key-point").waitFor();
  await page.getByRole("button", { name: "深入分析", exact: true }).click();
  await page.locator(".n-tabs-tab", { hasText: "笔记" }).click();
  const box = page.getByTestId("note-input").locator("textarea");
  await box.click();
  await box.type("先写下自己的想法");
  for (let i = 0; i < 120; i++) {
    const d = await bridge("node_detail", { req: { study_id: sid, node_id: 2 } });
    if (d.explanation?.budget === "deep" && !d.explanation.pending) break;
    await new Promise((r) => setTimeout(r, 500));
  }
  await page.waitForTimeout(500);
  check((await box.inputValue()) === "先写下自己的想法", "讲解更新后笔记内容没有被改动");
  check(await box.evaluate((el) => document.activeElement === el), "讲解更新后输入焦点仍在笔记中");
  check((await page.getByTestId("note-state").innerText()).includes("未保存的修改"), "笔记显示有未保存的修改");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}

// ---------- C. AI 讲解到达时保持阅读位置 ----------
{
  const { browser, page, problems } = await open({ width: 1440, height: 760 }, { detail: true });
  await page.locator('[data-node="1"]').click();
  await page.getByTestId("key-point").waitFor();
  await page.getByTestId("coach-generate").waitFor();
  const scroller = page.locator(".panel .n-scrollbar-container").first();
  const anchor = page.locator(".panel h4", { hasText: "候选比较" });
  // 读到详细分析中的候选比较
  await anchor.evaluate((el) => el.scrollIntoView({ block: "start" }));
  await scroller.evaluate((el) => el.scrollBy(0, -60));
  await page.waitForTimeout(300);
  const y0 = (await anchor.boundingBox()).y;
  const h0 = await scroller.evaluate((el) => el.scrollHeight);
  await bridge("coach_explain", { req: { study_id: sid, node_id: 1, audience: null, detail: null, refresh: false } });
  await page.getByTestId("coach-status").filter({ hasText: "已生成" }).waitFor({ state: "attached", timeout: 30000 });
  await page.waitForTimeout(400);
  const y1 = (await anchor.boundingBox()).y;
  const h1 = await scroller.evaluate((el) => el.scrollHeight);
  check(h1 !== h0, `AI 讲解改变了上方内容的高度（${h0} → ${h1}）`);
  check(Math.abs(y1 - y0) <= 4, `正在阅读的段落位置保持不变（${Math.round(y0)} → ${Math.round(y1)}）`);
  await scroller.evaluate((el) => el.scrollTo(0, 0));
  const lessons = await page.getByTestId("lesson").count();
  const card = await page.getByTestId("coach-card").innerText();
  check(lessons === 1 && !card.includes("学习要点"), "有效 AI 讲解出现后，学习点只出现一处");
  check(!card.includes("架起中炮，开始出动子力"), "AI 讲解中不再重复一句话摘要");
  await snap(page, "ai-reading");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}

// ---------- D. 分析不完整时，补充分析是主要操作 ----------
{
  const { browser, page, problems } = await launch({ width: 1440, height: 900 });
  await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
  // 把第 4 着的讲解改成“分析不完整”，模拟搜索未完成的结果
  const partial = (rec) => {
    if (rec?.node_id === 4 && rec.explanation) rec.explanation.level = "partial";
    return rec;
  };
  await page.route("**/__bridge/invoke/node_detail", async (route) => {
    const body = await (await route.fetch()).json();
    partial(body.explanation);
    await route.fulfill({ json: body });
  });
  await page.route("**/__bridge/invoke/explain_move", async (route) => {
    await route.fulfill({ json: partial(await (await route.fetch()).json()) });
  });
  await page.goto(BASE);
  await page.waitForSelector(".xq-board");
  await page.locator('[data-node="4"]').click();
  const alert = page.getByTestId("partial-alert");
  await alert.waitFor();
  check((await alert.innerText()).includes("只显示已确认的事实"), "说明只显示已确认的事实");
  const retry = alert.getByRole("button", { name: "补充分析" });
  check(await retry.evaluate((el) => el.classList.contains("n-button--warning-type")), "“补充分析”是这一区域的主要操作");
  if (await page.getByTestId("demo-reply").count()) check(!(await isPrimary(page.getByTestId("demo-reply"))), "此时“演示主要应对”降为次要操作");
  await snap(page, "partial");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}

// ---------- E. 分隔条的键盘操作 ----------
{
  const { browser, page, problems } = await open({ width: 1440, height: 900 });
  await page.locator('[data-node="3"]').click();
  const selected = () => page.evaluate(() => document.querySelector(".move.active")?.getAttribute("data-node"));
  const sp = page.getByTestId("splitter-left");
  const now = async () => Number(await sp.getAttribute("aria-valuenow"));
  check((await sp.getAttribute("aria-label")) === "调整棋谱栏宽度", "分隔条有可访问名称");
  await sp.focus();
  const w0 = await now();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  check((await now()) === w0 + 32, `方向键调整棋谱栏宽度（${w0} → ${await now()}）`);
  check((await selected()) === "3", "在分隔条上按方向键不会切换棋谱");
  check((await page.locator(".splitter .size").innerText()) === `${await now()} px`, "调整时显示当前宽度");
  check((await sp.getAttribute("aria-valuetext")).includes(String(await now())), "读屏可读到当前宽度");
  await page.keyboard.press("Home");
  check((await now()) === Number(await sp.getAttribute("aria-valuemin")), "Home 调到最小宽度");
  await page.keyboard.press("End");
  check((await now()) === Number(await sp.getAttribute("aria-valuemax")), "End 调到最大宽度");
  await page.keyboard.press("Enter");
  check((await now()) === 260, "Enter 恢复默认宽度");
  const box = await page.locator("#pane-tree").boundingBox();
  check(Math.abs(box.width - 260) <= 1, `棋谱栏实际宽度随之改变（${Math.round(box.width)}px）`);
  const right = page.getByTestId("splitter-right");
  await right.focus();
  const r0 = Number(await right.getAttribute("aria-valuenow"));
  await page.keyboard.press("ArrowLeft");
  check(Number(await right.getAttribute("aria-valuenow")) === r0 + 16, "讲解栏的分隔条向左移动时讲解栏变宽");
  await right.dblclick();
  check(Number(await right.getAttribute("aria-valuenow")) === 420, "双击恢复讲解栏默认宽度");
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}

// ---------- F. 窄窗口的操作，以及两种主题下的对比度 ----------
{
  const { browser, page, problems } = await open({ width: 760, height: 1000 });
  for (const btn of await page.getByTestId("board-controls").getByRole("button").all()) {
    const b = await btn.boundingBox();
    const name = (await btn.innerText()).trim() || (await btn.getAttribute("aria-label"));
    check(b && b.x >= 0 && b.x + b.width <= 760 && b.height >= 32, `窄窗口下“${name}”完整可见，高度 ${Math.round(b?.height ?? 0)}px`);
    // 禁用的按钮（如最后一着的“下一步”）只检查是否可见
    if (await btn.isEnabled()) await btn.click({ trial: true });
  }
  check(true, "窄窗口下可用的棋盘操作都可以点击");
  // 中等宽度下演示工具栏也保持一行
  await page.setViewportSize({ width: 1100, height: 860 });
  await page.getByTestId("toggle-tree").click();
  await page.getByTestId("tree-overlay").locator('[data-node="3"]').click();
  await page.keyboard.press("Escape");
  await page.getByTestId("demo-reply").click();
  await page.getByTestId("preview-next").click();
  check((await rowsOf(page.getByTestId("board-controls"))) === 1, "1100px 宽时演示工具栏保持一行");
  await browser.close();
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
}
for (const theme of ["light", "dark"]) {
  const { browser, page, problems } = await open({ width: 1440, height: 900 }, { theme, detail: true });
  await page.locator('[data-node="1"]').click();
  await page.getByTestId("coach-status").filter({ hasText: "已生成" }).waitFor({ timeout: 10000 });
  const targets = [
    ["程序依据", page.getByTestId("coach-basis").locator("li").first()],
    ["讲解正文", page.locator(".panel .claims .claim").first()],
    ["分析条件", page.locator(".panel .conditions").first()],
    ["学习点标题", page.locator(".lesson-title")],
    ["评价标签", page.locator(".head-tags .n-tag").first()],
    ["着法序号", page.locator(".move .num").first()],
    ["状态栏统计", page.getByTestId("explain-stats")],
  ];
  for (const [name, loc] of targets) {
    const r = await contrastOf(loc);
    check(r >= 4.5, `${theme === "dark" ? "深色" : "浅色"}主题下${name}的对比度 ${r.toFixed(2)}:1`);
  }
  await page.getByTestId("demo-reply").click();
  const bar = await contrastOf(page.getByTestId("explain-subject"));
  check(bar >= 4.5, `${theme === "dark" ? "深色" : "浅色"}主题下演示说明的对比度 ${bar.toFixed(2)}:1`);
  if (problems.length) {
    console.log(problems.join("\n"));
    process.exitCode = 1;
  }
  await browser.close();
}
console.log("  ✓ 场景结束");
