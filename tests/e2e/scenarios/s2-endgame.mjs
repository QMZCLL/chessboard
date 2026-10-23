// 场景 2：重启后恢复 → 残局摆棋（黑走）→ FEN 缺行棋方需选择 → 将死与终局分析 → 困毙 → 先想后讲 → 设置 1.5 秒。
import { BASE, bridge, check, launch, move, snap, waitText } from "../lib.mjs";

const { browser, page, problems } = await launch();
await page.goto(BASE);
await waitText(page, "建立学习局面");

// 1. 重启后恢复已保存的会话
await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
await page.locator(".saved-row", { hasText: "导入的棋谱" }).first().click();
await page.locator(".line:not(.variation) > .move", { hasText: "车一平二" }).waitFor();
const variations = await page.locator(".line.variation .move").count();
const detailedDots = await page.locator(".move .dot.detailed").count();
check(variations >= 1, `重启后分支仍在（${variations} 个分支着法）`);
check(detailedDots >= 5, `重启后讲解状态仍在（${detailedDots} 着已有详细讲解）`);
await snap(page, "restored-study");

// 2. 摆棋：只留将帅 → 粘贴 FEN → 改为黑走
await page.getByRole("button", { name: "新建 / 打开" }).click();
await page.locator(".n-tabs-tab", { hasText: "摆棋" }).click();
await page.getByPlaceholder("如 3k5/9/9/9/9/9/9/9/4R4/4K4 w").fill("3k5/9/9/9/9/9/9/9/4R4/4K4 w");
await page.getByRole("button", { name: "应用" }).click();
await page.locator(".n-radio-button", { hasText: "黑走" }).first().click();
await waitText(page, "局面成立");
await snap(page, "setup-black-to-move");
await page.getByRole("button", { name: "从这个局面开始" }).click();
await page.locator("[data-testid=board-context] .side.black").waitFor();
check(await page.locator("[data-testid=board-context] .side.black").innerText() === "黑方走", "残局按所选的黑方走开始");
await page.locator(".n-tabs-tab", { hasText: "局面分析" }).click();
const posSummary = await page.locator(".panel .position-summary").first().innerText();
console.log(`  局面说明：${posSummary}`);
check(posSummary.startsWith("轮到黑方走"), "局面说明给出当前行棋方");
check(await page.getByText("此局面之前的着法未知").count() > 0, "说明此前历史未知");
await page.getByTestId("analyze-position").click();
await page.locator(".panel .candidates .row").first().waitFor({ timeout: 30000 });
await snap(page, "endgame-black-analysis");

// 3. FEN 不含行棋方：必须选择
await page.getByRole("button", { name: "新建 / 打开" }).click();
await page.locator(".n-tabs-tab", { hasText: "FEN" }).click();
await page.getByPlaceholder("粘贴 FEN，如 3k5/9/9/9/9/9/9/9/4R4/4K4 w").fill("3k5/9/9/9/9/9/9/9/4R4/4K4");
await waitText(page, "FEN 没有写明行棋方");
check(await page.getByRole("button", { name: "导入" }).isDisabled(), "未选行棋方时不能导入");
await page.locator(".n-modal .n-radio-button", { hasText: "红走" }).first().click();
await page.getByRole("button", { name: "导入" }).click();
await page.locator("[data-testid=board-context] .side.red").waitFor();

// 4. 车五平六将死，终局局面不再导致崩溃
await move(page, "e1d1");
await page.locator("[data-testid=board-context] .n-tag", { hasText: "已被将死" }).waitFor();
check(true, "将死后棋盘提示“已被将死”");
await page.locator(".n-tabs-tab", { hasText: "本步讲解" }).click();
await page.locator(".panel .n-tag", { hasText: "引擎分析完整" }).waitFor({ timeout: 60000 });
const mateSummary = await page.getByTestId("key-point").innerText();
console.log(`  结论：${mateSummary}`);
check(mateSummary.includes("将死对方"), "讲解说明将死");
check(await page.locator(".panel .n-tag", { hasText: "当前分析的首选" }).count() === 1, "将死着法被评为首选");
await page.waitForTimeout(500);
const treeStats = await page.locator(".stats").first().innerText();
check(treeStats.includes("讲解 1/1") && !treeStats.includes("进行中"), `棋谱统计与讲解一致：${treeStats}`);
check(await page.locator(".move .dot.detailed").count() === 1, "棋谱中该着显示为已完成详细讲解");
await snap(page, "checkmate");
await page.locator(".n-tabs-tab", { hasText: "局面分析" }).click();
const mated = await page.locator(".panel .position-summary").first().innerText();
check(mated.includes("已被将死"), `终局局面说明：${mated}`);

// 5. 困毙
await page.getByRole("button", { name: "新建 / 打开" }).click();
await page.locator(".n-tabs-tab", { hasText: "FEN" }).click();
await page.getByPlaceholder("粘贴 FEN，如 3k5/9/9/9/9/9/9/9/4R4/4K4 w").fill("3k5/4R4/4R4/9/9/9/9/9/9/4K4 b");
await page.getByRole("button", { name: "导入" }).click();
await page.locator("[data-testid=board-context] .n-tag", { hasText: "困毙" }).waitFor();
check(!(await page.getByTestId("board-context").innerText()).includes("走子"), "终局局面不再提示走子");
check(await page.getByTestId("start-intro").count() === 0, "终局局面不显示“开始学习”的引导");
const stale = await page.locator(".panel .claims .claim").first().innerText();
console.log(`  困毙原因：${stale}`);
check(stale.includes("可走点都不安全"), "困毙说明列出将的可走点为何不安全");
await snap(page, "stalemate");

// 6. 先想后讲
await page.getByRole("button", { name: "新建 / 打开" }).click();
await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
await page.locator(".saved-row", { hasText: "导入的棋谱" }).first().click();
const mainMove = page.locator(".line:not(.variation) > .move", { hasText: "马8进7" });
await mainMove.waitFor();
await page.locator(".n-radio-button", { hasText: "先想后讲" }).click();
await mainMove.click();
await waitText(page, "先想一想");
check((await page.getByTestId("board-context").innerText()).includes("先想后讲"), "棋盘显示落子前局面并提示先思考");
await move(page, "b9c7");
await page.locator(".attempt", { hasText: "你的走法" }).waitFor();
await page.locator(".attempt .summary").waitFor({ timeout: 30000 });
check((await page.locator(".attempt .notation").innerText()) === "马2进3", "对照卡片显示用户试走“马2进3”");
await page.locator(".attempt .n-tag").nth(1).waitFor({ timeout: 60000 });
await page.locator(".line.variation .move", { hasText: "马2进3" }).locator(".dot.detailed").waitFor({ timeout: 30000 });
check(true, "试走分支在棋谱中显示为已完成详细讲解");
await snap(page, "think-first");
await page.locator(".n-radio-button", { hasText: "看谱学习" }).click();

// 7. 设置：1.5 秒 → 1500 毫秒
await page.getByRole("button", { name: "设置" }).click();
await page.locator(".n-drawer .n-tabs-tab", { hasText: "分析引擎" }).click();
// 选一个与当前不同的带小数的值，保证有修改可保存（可重复运行）
const before = (await bridge("app_status")).config.analysis.deep_time_ms;
const seconds = before === 1500 ? "2.5" : "1.5";
const deep = page.locator('[data-testid="deep-seconds"] input');
await deep.fill(seconds);
await deep.blur();
await page.locator('[data-testid="save-settings"]').click();
await waitText(page, "设置已保存");
const status = await bridge("app_status");
check(status.config.analysis.deep_time_ms === Number(seconds) * 1000, `界面 ${seconds} 秒 → 后台 ${status.config.analysis.deep_time_ms} 毫秒`);
await snap(page, "settings");
await page.keyboard.press("Escape");

// 8. 翻转棋盘
await page.getByRole("button", { name: "翻转棋盘" }).click();
await page.waitForTimeout(300);
await snap(page, "flipped");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
