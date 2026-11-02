// 场景 23：学习画像。首页按学习事件说明最近反复错在哪里（写明次数，不显示百分比）、今天最值得练什么（可以直接开始），
// 以及掌握的依据；可以打开出错的例子。
import { BASE, bridge, check, launch, snap } from "../lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const created = await bridge("create_study", {
  req: { name: "画像", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8 3. 车一平二 马2进3" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 200; i++) {
  if ((await bridge("study_view", { id: sid })).stats.detailed >= 6) break;
  await sleep(500);
}
const earlier = await bridge("current_practice");
if (earlier) await bridge("update_practice", { req: { id: earlier.id, index: null, skip: null, goal: null, finish: true } });
for (const node of [1, 2, 3, 4, 5, 6]) await bridge("add_review", { req: { study_id: sid, node_id: node } });
// 找一个至少有两张卡片的主题，在这些卡片上连续直接看答案
const items = (await bridge("list_reviews")).filter((i) => i.card.study_id === sid);
const byTheme = new Map();
for (const i of items) for (const t of i.themes) byTheme.set(t.theme, [...(byTheme.get(t.theme) ?? []), i.card.id]);
const [theme, cards] = [...byTheme.entries()].sort((a, b) => b[1].length - a[1].length)[0] ?? [];
check(!!theme && cards.length >= 2, `有两张以上卡片的主题：${theme}（${cards?.length} 张）`);
for (let i = 0; i < 6; i++) await bridge("review_attempt", { req: { id: cards[i % cards.length], choice: null, goal: "recall", request: `s23-${Date.now()}-${i}` } });
const profile = await bridge("learning_profile");
const label = profile.themes.find((t) => t.theme === theme).label;

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((s) => localStorage.setItem("xq-last-study", s), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
await page.getByTestId("open-home").click();
const block = page.getByTestId("home-profile");
await block.waitFor();
const weak = block.getByTestId("profile-weakness").filter({ hasText: label });
await weak.waitFor();
const text = await weak.innerText();
check(/最近 30 天涉及“.+”的 \d+ 次作答中，\d+ 次没有独立选对（.*直接看答案 \d+ 次.*）/.test(text), `写明反复出错的地方与次数：${text}`);
check(!(await block.innerText()).includes("%"), "画像不显示百分比");
const rec = await block.getByTestId("profile-recommendation").innerText();
check(/建议先练“.+”这一组（\d+ 张卡片）：最近 \d+ 次作答中 \d+ 次没有独立选对。/.test(rec), `推荐写明依据：${rec}`);
check((await block.getByTestId("profile-phases").innerText()).includes("开局"), "按阶段说明作答");
await snap(page, "profile");

// 打开一个例子：复习库打开这张卡片
await weak.getByTestId("profile-example").click();
const dialog = page.getByTestId("review-dialog");
await dialog.locator("[data-review].active").waitFor();
const opened = await dialog.locator("[data-review].active").getAttribute("data-review");
check(cards.includes(opened) || profile.weaknesses.some((w) => w.examples.includes(opened)), `打开出错的例子：${opened}`);
await page.keyboard.press("Escape");
await dialog.waitFor({ state: "hidden" });

// 练这一组：进入专注练习工作区，按推荐的主题开始
const recommended = profile.recommendation;
await block.getByTestId("profile-start").click();
const ws = page.getByTestId("practice-workspace");
await ws.getByTestId("practice-progress").waitFor();
const wsLabel = await ws.getByTestId("practice-label").innerText();
check(wsLabel.includes(recommended ? profile.themes.find((t) => t.theme === recommended.theme).label : label), `按推荐的主题开始练习：${wsLabel}`);
await ws.getByTestId("practice-exit").click();
await page.getByTestId("home").waitFor();

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
