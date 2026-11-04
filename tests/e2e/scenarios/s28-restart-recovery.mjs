// 场景 28：完整学习闭环经过强制结束、重启与换数据目录（自带 Pikafish）。
// 先想后讲没有选对 → 本局报告中出题 → 复习中作答 → 强制结束开发桥（SIGKILL）并重启 → 重新打开，内容与之前完全相同、
// 讲解不会一直显示进行中 → 继续作答 → 导出学习档案 → 换到新的空数据目录（像换了一台电脑）→ 从档案导入 → 内容与导出前相同。
import fs from "node:fs";
import path from "node:path";
import { BASE, OUT, bridge, check, launch, restartBridge, sameStored, snap, stored, transportNoise, until } from "../lib.mjs";

const ENDGAME = "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w";
const RECORD = "1. 炮二平五 马8进7 2. 马二进三 车9平8";
const settled = (id) => until(`会话 ${id} 的讲解完成`, async () => (await bridge("study_view", { id })).stats.pending === 0, 90000);
// 没有进行中的后台任务（比较保存的内容前，讲解等不会再改变记录）
const idle = () => until("后台任务都结束", async () => (await bridge("job_history")).active.length === 0, 90000);
// 不在后台补充深入分析：保存的讲解在比较前后不会改变
const config = (await bridge("app_status")).config;
await bridge("update_config", { config: { ...config, analysis: { ...config.analysis, deepen_unsettled: false } } });

// 残局：执红，正被将军时先想后讲没有选对，可以出一道找威胁
const endgame = (
  await bridge("create_study", {
    req: { name: "闭环残局", kind: { type: "fen", fen: ENDGAME, side: null }, orientation: null, user_side: "red" },
  })
).study.study.id;
const played = await bridge("apply_move", { req: { study_id: endgame, parent: 0, mv: "e2c3", variation: false } });
await settled(endgame);
await bridge("record_practice", {
  req: { study_id: endgame, node_id: played.node_id, hints_used: 0, revealed_directly: false, first_choice: "e2g3", hints: null },
});
// 对局：其中一着加入复习，复现棋谱
const game = (await bridge("import_text", { text: RECORD, name: "闭环对局" })).study.study.id;
await settled(game);
const firstMove = (await bridge("study_view", { id: game })).study.nodes["0"].children[0];
const moveCard = (await bridge("add_review", { req: { study_id: game, node_id: firstMove } })).card.id;

const { browser, page, problems } = await launch();
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), endgame);
await page.goto(BASE);
await page.waitForSelector(".xq-board");

// 出题
await page.getByRole("button", { name: "本局报告" }).click();
const report = page.getByTestId("report-dialog");
await report.getByTestId("report-generate").click();
const generated = await report.getByTestId("report-generated").innerText();
check(generated.startsWith("新增") && generated.includes("找威胁 1"), `本局报告中出题：${generated}`);
await page.keyboard.press("Escape");
await report.waitFor({ state: "hidden" });

// 复习中作答：找威胁点出将军的炮
await page.getByTestId("open-review").click();
const dialog = page.getByTestId("review-dialog");
const threatId = `${endgame}/0000000001/threat`;
await dialog.locator(`[data-review="${threatId}"]`).click();
await dialog.locator('[data-testid="review-card"][data-review-state="ready"]').waitFor();
await dialog.getByTestId("review-card").locator('[data-square="e7"]').first().click();
await dialog.getByTestId("review-submit-marks").click();
await dialog.getByTestId("review-result").waitFor();
check((await dialog.getByTestId("review-badge").innerText()) === "全部找到", "重启前完成一次作答");
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });
await idle();
const before = await stored();
check(before.cards.find((c) => c.id === threatId)?.attempts.length === 1, "作答已经保存");

// ---------- 强制结束并重启 ----------
await restartBridge({ signal: "SIGKILL" });
await page.reload();
await page.waitForSelector(".xq-board");
const after = await stored();
const diff = sameStored(before, after);
check(diff === "", `强制结束后重新打开，会话、讲解、卡片、作答与学习事件都与之前相同${diff ? `：${diff}` : ""}`);
await settled(endgame);
await settled(game);
check((await bridge("study_view", { id: endgame })).stats.pending === 0, "重新打开后讲解不会一直显示进行中");
await snap(page, "after-restart");

// 继续：复现对局中的第一着
await page.getByTestId("open-review").click();
await dialog.locator(`[data-review="${moveCard}"]`).click();
const card = dialog.getByTestId("review-card");
await dialog.locator('[data-testid="review-card"][data-review-state="ready"]').waitFor();
await card.locator('[data-square="h2"]').first().click();
await card.locator('[data-square="e2"]').first().click();
await dialog.getByTestId("review-result").waitFor();
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });
await idle();
const continued = await stored();
check(continued.cards.find((c) => c.id === moveCard)?.attempts.length === 1, "重启后继续作答并保存");
check(continued.events.length === before.events.length + 1, `重启后的作答记一条学习事件（${before.events.length} → ${continued.events.length}）`);

// ---------- 导出学习档案，换到新的空数据目录后导入 ----------
await page.getByRole("button", { name: "设置" }).click();
const drawer = page.locator(".n-drawer");
await drawer.locator(".n-tabs-tab", { hasText: "学习档案" }).click();
const panel = drawer.getByTestId("archive-panel");
const [download] = await Promise.all([page.waitForEvent("download"), panel.getByTestId("archive-export").click()]);
const file = path.join(OUT, "s28-archive.json");
await download.saveAs(file);
check(JSON.parse(fs.readFileSync(file, "utf8")).format === "xqlink-archive", "导出学习档案");
await page.keyboard.press("Escape");

await restartBridge({ fresh: true });
check((await bridge("list_studies")).length === 0, "新的数据目录中没有任何学习数据");
await page.reload();
// 没有学习数据时先显示“建立学习局面”：关掉它，从设置中导入学习档案
await page.getByText("建立学习局面").first().waitFor();
await page.keyboard.press("Escape");
await page.locator(".n-modal-mask").waitFor({ state: "detached" });
await page.getByRole("button", { name: "设置" }).click();
await drawer.locator(".n-tabs-tab", { hasText: "学习档案" }).click();
await panel.getByTestId("archive-file").setInputFiles(file);
await panel.getByTestId("archive-preview").waitFor();
await panel.getByTestId("archive-confirm").click();
await panel.getByTestId("archive-preview").waitFor({ state: "detached" });
const imported = await stored();
const moved = sameStored(continued, imported);
check(moved === "", `在新的数据目录中导入后，内容与导出前相同${moved ? `：${moved}` : ""}`);
await snap(page, "after-import");

const real = problems.filter((p) => !transportNoise(p));
check(real.length === 0, `控制台无错误${real.length ? `：${real.join("；")}` : ""}`);
await browser.close();
