// 场景 18：到期复习、每日练习与学习档案。
// A. 复习入口与“今天”分组按下一次复习的时间：新加入的卡片立即到期；
// B. 今天的练习：按顺序复习，标记后说明下一次复习的时间（独立选对后标为已理解拉长间隔，直接看答案或没有选对时明天再看），
//    结束后的小结来自实际记录；只练需要再练的卡片；
// C. 设置中的复习间隔；
// D. 学习档案：导出、导入前预览、恢复删除的卡片（历次复习与安排一起恢复）、立即备份。
import fs from "node:fs";
import path from "node:path";
import { BASE, OUT, bridge, check, launch, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const created = await bridge("create_study", {
  req: { name: "每日练习", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 3) break;
  await sleep(500);
}
for (const i of await bridge("list_reviews")) await bridge("delete_review", { req: { id: i.card.id } });
const ids = [];
for (const node of [1, 2, 3]) {
  ids.push((await bridge("add_review", { req: { study_id: sid, node_id: node } })).card.id);
  await sleep(5);
}
const cardOf = async (id) => (await bridge("list_reviews")).find((i) => i.card.id === id).card;
const DAY = 86_400_000;
const daysUntil = (card) => Math.round((card.due_at - Date.now()) / DAY);

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const dialog = page.getByTestId("review-dialog");
const reviewCard = dialog.getByTestId("review-card");
async function reviewMove(mv) {
  for (const sq of [mv.slice(0, 2), mv.slice(2, 4)]) await reviewCard.locator(`[data-square="${sq}"]`).first().dispatchEvent("mousedown", { button: 0 });
}
const todayCount = async () => Number(((await page.getByTestId("open-review").getAttribute("aria-label")) ?? "").match(/今天 (\d+) 张到期/)?.[1] ?? 0);
/** 标记状态，返回这次标记的提示（之前的提示可能还没消失，等新的一条出现） */
async function mark(which) {
  const messages = page.locator(".n-message");
  const before = await messages.count();
  await dialog.getByTestId(`review-mark-${which}`).click();
  await messages.nth(before).waitFor();
  return (await messages.nth(before).innerText()).trim();
}

// ---------- A. 今天到期 ----------
check((await todayCount()) === 3, "新加入的 3 张卡片立即到期，顶部显示今天到期的张数");
await page.getByTestId("open-review").click();
await dialog.getByTestId("review-filter-today").waitFor();
check((await dialog.getByTestId("review-filter-today").innerText()).includes("3"), "“今天”分组有 3 张");
check((await dialog.locator("[data-review]").first().innerText()).includes("今天到期"), "列表中说明下一次复习的时间");

// ---------- B. 今天的练习 ----------
await dialog.getByTestId("review-start-today").click();
const session = dialog.getByTestId("review-session-progress");
await session.waitFor();
check((await session.innerText()).includes("今天的练习 · 第 1 / 3 张"), `开始一组练习：${(await session.innerText()).replace(/\n/g, " ")}`);
await dialog.locator(`[data-review="${ids[0]}"].active`).waitFor();
// 第 1 张（炮二平五）：走对，标为已理解
await reviewMove("h2e2");
await dialog.getByTestId("review-result").waitFor();
let notice = await mark("understood");
check(notice.includes("已理解") && notice.includes("3 天后再看"), `独立选对后标为已理解，间隔拉长：${notice}`);
check((await dialog.getByTestId("review-due").innerText()) === "3 天后再看", "卡片上显示下一次复习的时间");
check((await dialog.getByTestId("review-next").innerText()).includes("下一张（2 / 3）"), "练习中按顺序进入下一张");
await dialog.getByTestId("review-next").click();
await dialog.locator(`[data-review="${ids[1]}"].active`).waitFor();
// 第 2 张：直接看答案后标为已理解
await dialog.getByTestId("review-reveal").click();
await dialog.getByTestId("review-result").waitFor();
notice = await mark("understood");
check(notice.includes("明天再看"), `直接看答案后标为已理解：明天再看（${notice}）`);
await dialog.getByTestId("review-next").click();
await dialog.locator(`[data-review="${ids[2]}"].active`).waitFor();
// 第 3 张（马二进三）：走了马八进七，标为需要再练
await reviewMove("b0c2");
await dialog.getByTestId("review-result").waitFor();
notice = await mark("again");
check(notice.includes("需要再练") && notice.includes("明天再看"), `标为需要再练：${notice}`);
check((await dialog.getByTestId("review-next").innerText()).includes("完成这组练习"), "最后一张之后完成这组练习");
await dialog.getByTestId("review-next").click();
const summary = await dialog.getByTestId("review-session-summary").innerText();
check(
  summary === "本次完成 3 张：1 张独立选对，1 张没有选对，1 张直接看了答案。安排：明天再看 2 张，3 天后再看 1 张。",
  `小结来自实际记录：${summary}`,
);
await snap(page, "session-summary");
const [c1, c2, c3] = await Promise.all(ids.map(cardOf));
check(
  [daysUntil(c1), daysUntil(c2), daysUntil(c3)].join() === "3,1,1" && c1.stage === 1 && c3.status === "again",
  `后台保存的安排：${[daysUntil(c1), daysUntil(c2), daysUntil(c3)].join("、")} 天`,
);
check((await todayCount()) === 0 && (await dialog.getByTestId("review-filter-today").innerText()).includes("0"), "今天到期的都已练完");
await dialog.getByTestId("review-close-summary").click();
// 只练需要再练的卡片
await dialog.getByTestId("review-start-again").click();
await session.waitFor();
check((await session.innerText()).includes("需要再练的卡片 · 第 1 / 1 张"), "只练需要再练的卡片");
await dialog.locator(`[data-review="${ids[2]}"].active`).waitFor();
await dialog.getByTestId("review-finish").click();
check((await dialog.getByTestId("review-session-summary").innerText()) === "这组 1 张还没有复习。", "提前结束时如实说明");
await dialog.getByTestId("review-close-summary").click();
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });

// ---------- C. 设置中的复习间隔 ----------
const originalReview = (await bridge("app_status")).config.review;
await page.getByRole("button", { name: "设置" }).click();
const drawer = page.locator(".n-drawer");
await drawer.getByTestId("review-intervals").locator("input").fill("2, 5, 9");
await drawer.getByTestId("save-settings").click();
await drawer.getByTestId("settings-state").filter({ hasText: "所有修改已保存" }).waitFor();
check((await bridge("app_status")).config.review.intervals_days.join() === "2,5,9", "设置中修改复习间隔");

// ---------- D. 学习档案 ----------
await drawer.locator(".n-tabs-tab", { hasText: "学习档案" }).click();
const panel = drawer.getByTestId("archive-panel");
await panel.waitFor();
const [download] = await Promise.all([page.waitForEvent("download"), panel.getByTestId("archive-export").click()]);
const file = path.join(OUT, "s18-archive.json");
await download.saveAs(file);
const archive = JSON.parse(fs.readFileSync(file, "utf8"));
check(
  archive.format === "xqlink-archive" && archive.studies.some((s) => s.study.id === sid) && archive.reviews.length === 3,
  `导出的学习档案包含会话与复习卡片（${archive.studies.length} 个会话、${archive.reviews.length} 张卡片）`,
);
// 删除一张卡片后从档案恢复：先预览，确认后导入
await bridge("delete_review", { req: { id: ids[0] } });
await panel.getByTestId("archive-file").setInputFiles(file);
const preview = panel.getByTestId("archive-preview");
await preview.waitFor();
const previewText = await preview.innerText();
check(
  previewText.includes("学习会话：新增 0 个；本机已有") && previewText.includes("复习卡片：新增 1 张；与本机相同 2 张；需要合并 0 张"),
  `导入前预览：${previewText.replace(/\n/g, " / ")}`,
);
check((await bridge("list_reviews")).length === 2, "预览不改动本机的数据");
await snap(page, "archive-preview");
await panel.getByTestId("archive-confirm").click();
await preview.waitFor({ state: "detached" });
const restored = await cardOf(ids[0]);
check(restored.attempts.length === 1 && restored.status === "understood" && daysUntil(restored) === 3, "删除的卡片连同复习记录与安排一起恢复");
// 立即备份
await panel.getByTestId("archive-backup-now").click();
const files = panel.getByTestId("archive-backups");
await files.waitFor();
check(/xqlink-archive-\d{4}-\d{2}-\d{2}\.json/.test(await files.innerText()), `立即备份：${(await files.innerText()).split("\n")[0]}`);
check((await panel.getByTestId("archive-backup-dir").innerText()).includes("backups"), "说明备份的位置");

await bridge("update_config", { config: { ...(await bridge("app_status")).config, review: originalReview } });

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
