// 场景 26：复盘收件箱。设置中填写“我的名字”；粘贴多盘棋谱预览：按名字判断我方并写明依据，标出与已有的棋局重复的，
// 比对不上的要先选我方才能导入；导入后首页列出每盘的进度，分析完后一次出题、写明新增几道，再出题时说明都出过题了；
// 可以打开本局报告、移出收件箱（棋局保留）。
import { BASE, bridge, check, launch, snap } from "../lib.mjs";

// 与同一组其他场景的棋局都不同（同组场景共用一个数据目录，相同的棋局会被认作重复）
const old = await bridge("import_text", { text: '[Event "s26 旧局"]\n1. 兵三进一 卒3进1', name: null });
const pasted = [
  '[Red "测试者"]\n[Black "对手甲"]\n[Date "2026.10.20"]\n1. 炮八平六 马2进3 2. 马八进七 车1平2 3. 车九平八 炮8平5',
  '[Red "对手乙"]\n[Black "测试者"]\n[Date "2026.10.18"]\n1. 相三进五 炮8平5 2. 马二进三 马8进7',
  '[Event "s26 练习"]\n1. 兵七进一 卒7进1 2. 马八进七 马2进3',
  '[Event "s26 重复"]\ng3g4 c6c5',
].join("\n\n");

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), old.study.study.id);
await page.goto(BASE);
await page.waitForSelector(".xq-board");

// 设置中填写“我的名字”
await page.getByRole("button", { name: "设置" }).click();
await page.locator('[data-testid="player-names"] input').fill("测试者、Tester");
await page.locator('[data-testid="save-settings"]').click();
await page.getByText("设置已保存").first().waitFor();
check(JSON.stringify((await bridge("app_status")).config.player.names) === '["测试者","Tester"]', "“我的名字”按顿号分开保存");
await page.keyboard.press("Escape");

await page.getByTestId("open-home").click();
const panel = page.getByTestId("home-inbox");
await panel.waitFor();
await panel.getByTestId("inbox-open-import").click();
const dialog = page.getByTestId("inbox-dialog");
await dialog.getByTestId("inbox-text").locator("textarea").fill(pasted);
await dialog.getByTestId("inbox-preview").click();
const row = (i) => dialog.locator(`[data-inbox-candidate="${i}"]`);
await row(3).waitFor();
const reasons = await dialog.getByTestId("inbox-reason").allInnerTexts();
check(reasons[0] === "红方“测试者”与设置中的名字相同" && reasons[1] === "黑方“测试者”与设置中的名字相同", `按名字判断我方并写明依据：${reasons.slice(0, 2).join("；")}`);
check(reasons[2] === "棋谱中没有写明双方棋手，请选择", `比对不上时说明需要选择：${reasons[2]}`);
const dup = await row(3).getByTestId("inbox-duplicate").innerText();
check(dup === "与已有的棋局“s26 旧局”相同，默认不导入", `记谱方式不同仍认出与已有的棋局重复：${dup}`);
check((await row(3).getByRole("checkbox").getAttribute("aria-checked")) === "false", "重复的默认不勾选");
check(await dialog.getByTestId("inbox-import").isDisabled(), "还有一盘没有选我方时不能导入");
check((await dialog.getByTestId("inbox-missing-side").innerText()) === "还有 1 盘没有选择我方", "写明还有几盘没有选择我方");
await snap(page, "inbox-preview");
await row(2).getByTestId("inbox-side-black").click();
await dialog.getByTestId("inbox-import").filter({ hasText: "加入收件箱（3 盘）" }).click();
await dialog.waitFor({ state: "hidden" });

// 首页列出每盘的进度；分析完后可以出题
const items = panel.locator("[data-inbox-item]");
await items.nth(2).waitFor();
const names = await items.locator(".name").allInnerTexts();
check(names[0].startsWith("测试者 对 对手甲") && names[0].includes("执红") && names[1].includes("执黑") && names[2].startsWith("s26 练习"), `按日期排列，写明我执哪一方：${names.join(" / ")}`);
const generate = panel.getByTestId("inbox-generate");
await generate.filter({ hasText: "为分析完的 3 盘出题" }).waitFor({ timeout: 180000 });
const progress = await panel.getByTestId("inbox-progress").allInnerTexts();
check(progress.every((p) => /^已分析 \d+ 着/.test(p)), `分析完后写明着数与关键局面：${progress.join(" / ")}`);
check((await panel.getByTestId("inbox-summary").innerText()) === "共 3 盘，3 盘已分析完，其中 3 盘可以出题。", "收件箱的一句话说明");
await snap(page, "inbox-analysed");
await generate.click();
const generated = panel.getByTestId("inbox-generated");
await generated.waitFor();
const text = await generated.innerText();
check(/^为 3 盘出题，(新增 \d+ 道：找威胁 \d+、二选一 \d+、找更好的着法 \d+|没有新的题目)/.test(text), `出题写明新增几道：${text}`);
check((await panel.getByTestId("inbox-exercises").count()) === 3, "每盘写明出题的结果");
check(await generate.isDisabled(), "都出过题后按钮不可用");
const again = await bridge("inbox_generate");
check(again.text === "没有需要出题的棋局：已经分析完的都出过题了。", `再出题时说明都出过了：${again.text}`);

// 打开本局报告；移出收件箱后棋局仍在
await items.nth(0).getByTestId("inbox-report").click();
await page.getByTestId("report-dialog").waitFor();
await page.keyboard.press("Escape");
await page.getByTestId("open-home").click();
const third = await items.nth(2).getAttribute("data-inbox-item");
await items.nth(2).getByTestId("inbox-remove").click();
await page.locator(".n-popconfirm").getByRole("button", { name: "确认" }).click();
await panel.locator(`[data-inbox-item="${third}"]`).waitFor({ state: "detached" });
check((await bridge("list_studies")).some((s) => s.id === third), "移出收件箱不删除棋局");
await snap(page, "inbox-after");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
