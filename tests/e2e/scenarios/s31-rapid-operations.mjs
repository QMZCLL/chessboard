// 场景 31：快速连续的操作与竞态（自带 Pikafish）。
// A. 连续点击“生成 AI 讲解”：只发出一次请求，最后显示已生成。
// B. 走子后立即删除这一着：不留下这一着的讲解，任务登记中这一着的讲解已经结束（取消或在删除前完成）。
// C. 讲解进行中切换到另一个会话：显示另一个会话的内容；切回来时原来那着的讲解已经完成。
// D. 快速打开、关闭复习：没有报错。
// E. 提交作答后立即换到另一张卡片：作答记在原来那张卡片上。
// F. 重新出题与提交作答同时发生：作答保留，题目不重复。
// G. 导入棋谱的同时打开另一个会话：两边都正常完成。
import { BASE, LLM, bridge, check, launch, llmCalls, llmMode, move, snap, until } from "../lib.mjs";

const ENDGAME = "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w";
const settled = (id) => until(`会话 ${id} 的讲解完成`, async () => (await bridge("study_view", { id })).stats.pending === 0, 90000);
const idle = () => until("后台任务都结束", async () => (await bridge("job_history")).active.length === 0, 90000);
const create = async (name, kind, user_side = null) =>
  (await bridge("create_study", { req: { name, kind, orientation: null, user_side } })).study.study.id;

// 准备：一盘对局（AI 讲解、删除、切换用），一盘执红的残局（出题与作答用）
const game = (await bridge("import_text", { text: "1. 炮二平五 马8进7 2. 马二进三", name: "快速操作对局" })).study.study.id;
const endgame = await create("快速操作残局", { type: "fen", fen: ENDGAME, side: null }, "red");
const played = await bridge("apply_move", { req: { study_id: endgame, parent: 0, mv: "e2c3", variation: false } });
await settled(game);
await settled(endgame);
await bridge("record_practice", {
  req: { study_id: endgame, node_id: played.node_id, hints_used: 0, revealed_directly: false, first_choice: "e2g3", hints: null },
});
await bridge("generate_exercises", { req: { study_id: endgame, regenerate: false } });
const threatId = `${endgame}/0000000001/threat`;
const recallId = (await bridge("add_review", { req: { study_id: game, node_id: 1 } })).card.id;
const status = await bridge("app_status");
await bridge("update_config", {
  config: { ...status.config, coach: { ...status.config.coach, enabled: true, base_url: LLM, model: "mock-model" } },
});
await bridge("set_coach_credential", { req: { key: "sk-e2e-rapid-0000" } });
await fetch(`${LLM}/__reset`, { method: "POST" });
await idle();

const { browser, page, problems } = await launch();
// 每一部分结束时检查这一部分的控制台错误，出错时能看出是哪一部分
let mark = 0;
const quietSince = (part) => {
  const fresh = problems.slice(mark);
  mark = problems.length;
  check(fresh.length === 0, `${part}：控制台无错误${fresh.length ? `：${fresh.join("；")}` : ""}`);
};
await page.addInitScript(() => {
  const s = window.localStorage.getItem("e2e-study");
  if (s) window.localStorage.setItem("xq-last-study", s);
});
const open = async (id) => {
  await page.evaluate((s) => localStorage.setItem("e2e-study", s), id);
  await page.reload();
  await page.waitForSelector(".xq-board");
};
await page.goto(BASE);
await open(game);

// ---------- A. 连续点击生成 AI 讲解 ----------
await llmMode({ mode: "valid", delay_ms: 800, script: [] });
await page.locator(".move", { hasText: "炮二平五" }).click();
await page.getByTestId("coach-generate").click({ clickCount: 3 });
await page.getByTestId("coach-status").filter({ hasText: "已生成" }).waitFor({ timeout: 30000 });
check((await llmCalls()).length === 1, `连续点击只发出一次请求（${(await llmCalls()).length} 次）`);
await llmMode({ mode: "valid", delay_ms: 0, script: [] });

quietSince("A");

// ---------- B. 走子后立即删除这一着 ----------
// 界面刚选中的这一着在别处被删除：选中之前发出的读取会得到 400（浏览器会记一条网络错误），界面忽略这个过期的结果
const problemsBeforeDelete = problems.length;
const last = Math.max(...Object.keys((await bridge("study_view", { id: game })).study.nodes).map(Number));
await page.locator(".move", { hasText: "马二进三" }).click();
await move(page, "b9c7");
const added = await until("新的一着", async () => {
  const nodes = (await bridge("study_view", { id: game })).study.nodes;
  return Object.values(nodes).find((n) => n.id > last && n.mv === "b9c7");
});
await bridge("delete_node", { req: { study_id: game, node_id: added.id } });
await idle();
// 界面收到删除后把选中的着法换成它的上一着；之后不会再读取已删除的着法
await until("界面换掉已删除的着法", async () =>
  (await page.evaluate(() => document.querySelector(".move.active")?.getAttribute("data-node"))) !== String(added.id),
);
await page.locator(".move.active", { hasText: "马二进三" }).waitFor();
const view = await bridge("study_view", { id: game });
check(!(String(added.id) in view.explain_status), "删除的着法没有留下讲解");
check(Object.keys(view.explain_status).every((n) => n in view.study.nodes), "每一条讲解都属于还在的着法");
const job = (await bridge("job_history")).finished.filter((j) => j.kind === "explain" && j.key === `explain:${game}:${added.id}`);
check(job.length > 0 && job.every((j) => ["cancelled", "succeeded", "superseded"].includes(j.state)), `这一着的讲解任务已经结束（${job.map((j) => j.state).join("、")}）`);
check(!(await page.getByText("棋谱中没有节点").count()), "界面上没有留下读取已删除着法的错误");
const stale = (p) => /node_detail|explain_move|status of 400/.test(p);
const deleteNoise = problems.splice(problemsBeforeDelete).filter((p) => !stale(p));
problems.push(...deleteNoise);
mark = Math.min(mark, problems.length);

quietSince("B");

// ---------- C. 讲解进行中切换到另一个会话 ----------
const other = await create("切换目标", { type: "standard" });
await bridge("apply_move", { req: { study_id: other, parent: 0, mv: "c3c4", variation: false } });
await settled(other);
const fresh = await create("切换来源", { type: "fen", fen: "4k4/9/9/9/9/9/9/9/3R5/3K5 w", side: null });
await open(fresh);
await move(page, "d1d8");
await page.getByRole("button", { name: /新建 \/ 打开/ }).click();
await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
await page.locator(".saved-main", { hasText: "切换目标" }).click();
await page.locator(".move", { hasText: "兵七进一" }).waitFor();
check(await page.locator(".move", { hasText: "兵七进一" }).isVisible(), "切换后显示另一个会话的棋谱");
await settled(fresh);
await open(fresh);
await page.getByTestId("key-point").waitFor();
check((await page.getByTestId("explain-pending").count()) === 0, "切回原来的会话时，那着的讲解已经完成");

quietSince("C");

// ---------- D. 快速打开、关闭复习 ----------
await open(endgame);
for (let i = 0; i < 5; i++) {
  await page.getByTestId("open-review").click();
  await page.keyboard.press("Escape");
}
await page.getByTestId("review-dialog").waitFor({ state: "hidden" });
check(true, "快速打开、关闭复习五次");

quietSince("D");

// ---------- E. 提交作答后立即换到另一张卡片 ----------
await page.getByTestId("open-review").click();
const dialog = page.getByTestId("review-dialog");
await dialog.locator(`[data-review="${threatId}"]`).click();
await dialog.locator('[data-testid="review-card"][data-review-state="ready"]').waitFor();
await dialog.getByTestId("review-card").locator('[data-square="e7"]').first().click();
await dialog.getByTestId("review-submit-marks").click();
await dialog.locator(`[data-review="${recallId}"]`).click();
await dialog.locator('[data-testid="review-card"][data-review-state="ready"]').waitFor();
const cards = async () => Object.fromEntries((await bridge("list_reviews")).map((i) => [i.card.id, i.card]));
await until("作答保存", async () => (await cards())[threatId].attempts.length === 1);
check((await cards())[recallId].attempts.length === 0, "作答记在原来那张卡片上，换到的卡片没有作答");

quietSince("E");

// ---------- F. 重新出题与提交作答同时发生 ----------
const before = Object.keys(await cards()).length;
await dialog.locator(`[data-review="${threatId}"]`).click();
await dialog.locator('[data-testid="review-card"][data-review-state="ready"]').waitFor();
const again = dialog.getByRole("button", { name: "再试一次" });
if (await again.count()) await again.first().click();
await dialog.getByTestId("review-card").locator('[data-square="e7"]').first().click();
await Promise.all([
  dialog.getByTestId("review-submit-marks").click(),
  bridge("generate_exercises", { req: { study_id: endgame, regenerate: true } }),
]);
await until("第二次作答保存", async () => (await cards())[threatId].attempts.length === 2);
check(Object.keys(await cards()).length === before, "重新出题没有多出重复的题目");
check((await cards())[threatId].attempts.length === 2, "同时重新出题时作答照常保留");
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });

quietSince("F");

// ---------- G. 导入棋谱的同时打开另一个会话 ----------
await page.getByRole("button", { name: /新建 \/ 打开/ }).click();
await page.locator(".n-tabs-tab", { hasText: "已保存" }).click();
const [imported] = await Promise.all([
  bridge("import_text", { text: "1. 相三进五 马8进7", name: "同时导入" }),
  page.locator(".saved-main", { hasText: "快速操作对局" }).click(),
]);
await page.locator(".move", { hasText: "炮二平五" }).waitFor();
check(!!imported.study.study.id, "导入同时完成");
check((await bridge("list_studies")).some((s) => s.name === "同时导入"), "导入的棋局已经保存");
await snap(page, "rapid-done");

quietSince("G");
await browser.close();
