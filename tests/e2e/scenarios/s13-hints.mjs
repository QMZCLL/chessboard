// 场景 13：先想后讲的分级提示与训练状态。
// A. 训练状态：正在思考的这一着及其后续着法在整个界面上都不透露，局面分析与笔记暂不可用；
// B. 分级提示：一级只说局面需求，二级在棋盘上标出相关棋子，三级给出不分先后的着法，点一步可查看箭头；
// C. 练习记录：用了几级提示、是否直接看讲解、第一步是否与棋谱相同，随会话保存。
import { BASE, bridge, check, launch, move, snap } from "../lib.mjs";

const created = await bridge("create_study", {
  req: { name: "先想后讲提示", kind: { type: "record", text: "1. 炮二平五 马8进7 2. 马二进三 车9平8" }, orientation: null, user_side: null },
});
const sid = created.study.study.id;
for (let i = 0; i < 160; i++) {
  const v = await bridge("study_view", { id: sid });
  if (v.stats.detailed >= 4) break;
  await new Promise((r) => setTimeout(r, 500));
}

const { browser, page, problems } = await launch({ width: 1440, height: 900 });
await page.addInitScript((id) => localStorage.setItem("xq-last-study", id), sid);
await page.goto(BASE);
await page.waitForSelector(".xq-board");
const notationOf = async (id) => (await page.locator(`[data-node="${id}"] .notation`).innerText()).trim();
const bodyText = () => page.evaluate(() => document.body.innerText);
const practiceOf = async (id) => (await bridge("study_view", { id: sid })).study.nodes[String(id)].practice;

// ---------- A. 训练状态 ----------
await page.getByText("先想后讲", { exact: true }).click();
await page.locator('[data-node="2"]').click();
await page.getByTestId("think-card").waitFor();
check((await notationOf(1)) === "炮二平五", "已经走过的着法照常显示");
check((await notationOf(2)).includes("？？？") && (await notationOf(3)).includes("？？？") && (await notationOf(4)).includes("？？？"), "正在思考的这一着及其后续着法在棋谱中隐藏");
check(
  (await page.locator(".n-tabs-tab--disabled", { hasText: "局面分析" }).count()) === 1 && (await page.locator(".n-tabs-tab--disabled", { hasText: "笔记" }).count()) === 1,
  "揭示前局面分析与笔记暂不可用",
);
let text = await bodyText();
check(!["马8进7", "马二进三", "车9平8"].some((n) => text.includes(n)), "整个界面上都没有透露这一着与后续着法");
check((await page.getByTestId("hint-count").innerText()).includes("0 / 3"), "开始时没有使用提示");

// ---------- B. 分级提示 ----------
const next = page.getByTestId("hint-next");
check(await next.evaluate((el) => el.classList.contains("n-button--primary-type")), "“给我一点提示”是主要操作");
check((await next.innerText()).includes("给我一点提示"), "第一级提示的入口");
await next.click();
const hints = page.getByTestId("hints");
const level1 = await hints.innerText();
check(level1.includes("提示 1 · 局面需求") && !level1.includes("提示 2"), `一级提示只说局面需求：${level1.replace(/\n/g, " ")}`);
check((await page.locator(".xq-board circle.hl").count()) === 0, "一级提示不在棋盘上标出棋子");
await next.click();
const level2 = await hints.innerText();
check(level2.includes("提示 2 · 相关棋子"), `二级提示指出相关棋子：${level2.split("提示 2 · 相关棋子")[1]?.replace(/\n/g, " ").slice(0, 40)}`);
check((await page.locator(".xq-board circle.hl").count()) > 0, "二级提示在棋盘上标出相关棋子");
text = await bodyText();
check(!text.includes("马8进7"), "一、二级提示没有透露棋谱中的着法");
await snap(page, "hint-2");
await next.click();
const chips = page.locator(".hint-move");
check((await chips.count()) >= 1 && (await hints.innerText()).includes("不分先后"), `三级提示给出不分先后的着法：${(await chips.allInnerTexts()).join("、")}`);
check((await page.getByTestId("hint-next").count()) === 0, "三级提示之后不再有更多提示");
await chips.first().click();
check((await page.locator(".xq-board line.arrow.next").count()) === 1, "点一步提示中的着法，在棋盘上显示它的走法");
check((await page.getByTestId("hint-count").innerText()).includes("3 / 3"), "记下用了三级提示");
await snap(page, "hint-3");

// ---------- C. 练习记录 ----------
// 走出与棋谱不同的一步（马2进3）
await move(page, "b9c7");
await page.getByTestId("practice").waitFor({ timeout: 30000 });
let p = await page.getByTestId("practice").innerText();
check(p.includes("用了 3 级提示") && p.includes("与棋谱不同"), `揭示后显示练习记录：${p}`);
check((await notationOf(2)) === "马8进7", "揭示后棋谱显示这一着");
check((await notationOf(3)).includes("？？？") && (await notationOf(4)).includes("？？？"), "这一着揭示后，后续着法仍然隐藏，等思考后再显示");
check((await page.locator(".variation .notation", { hasText: "马2进3" }).count()) === 1, "自己走出的试走着法照常显示");
let rec = await practiceOf(2);
check(rec?.hints_used === 3 && rec.first_choice === "b9c7" && rec.matched === false && !rec.revealed_directly, "练习记录保存在会话中");

// 不走子直接看讲解
await page.locator('[data-node="3"]').click();
await page.getByTestId("think-card").waitFor();
await page.getByTestId("reveal").click();
await page.getByTestId("practice").waitFor();
p = await page.getByTestId("practice").innerText();
check(p.includes("没有用提示") && p.includes("没有走子就看了讲解"), `直接看讲解也有记录：${p}`);
rec = await practiceOf(3);
check(rec?.revealed_directly === true && rec.first_choice === null, "记为直接揭示");

// 走出与棋谱相同的一步（车9平8）
await page.locator('[data-node="4"]').click();
await page.getByTestId("think-card").waitFor();
await move(page, "i9h9");
await page.getByTestId("practice").waitFor({ timeout: 30000 });
p = await page.getByTestId("practice").innerText();
check(p.includes("与棋谱相同"), `走对时记为与棋谱相同：${p}`);
check((await practiceOf(4))?.matched === true, "保存为与棋谱相同");

// 回到看谱学习：全部着法照常显示
await page.getByText("看谱学习", { exact: true }).click();
check((await notationOf(3)) === "马二进三" && (await notationOf(4)) === "车9平8", "回到看谱学习后棋谱照常显示");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
