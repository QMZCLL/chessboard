// 场景 7：AI 讲解。通过设置界面配置模型服务与密钥，生成讲解；点击段落只选中并高亮依据，演示用独立按钮；
// 密钥不出现在任何返回界面的数据中；模型输出不合格时保留原结果与模板讲解。
import { BASE, bridge, check, launch, snap, waitText } from "../lib.mjs";

const LLM = process.env.MOCK_LLM ?? "http://127.0.0.1:1440";
const SECRET = "sk-e2e-secret-1234";
// 从未配置的状态开始（可重复运行）
const status = await bridge("app_status");
await bridge("update_config", {
  config: { ...status.config, coach: { ...status.config.coach, enabled: false, base_url: "", model: "", audience: "beginner", detail: "normal" } },
});
await bridge("set_coach_credential", { req: { key: null } });
await fetch(`${LLM}/__reset`, { method: "POST" });
const { browser, page, problems } = await launch();
const bodies = [];
page.on("response", async (r) => {
  if (r.url().includes("/__bridge/")) bodies.push(await r.text().catch(() => ""));
});
await page.goto(BASE);
await waitText(page, "建立学习局面");
await page.locator(".n-tabs-tab", { hasText: "棋谱" }).click();
await page.getByPlaceholder("粘贴棋谱").fill("1. 炮二平五");
await page.getByRole("button", { name: "导入棋谱" }).click();
await page.locator(".move", { hasText: "炮二平五" }).click();
await page.locator(".panel .n-tag", { hasText: "引擎分析完整" }).waitFor({ timeout: 30000 });
const card = page.getByTestId("coach-card");
await card.waitFor();
check((await page.getByTestId("coach-status").innerText()).includes("可开启 AI 讲解"), "未配置时 AI 讲解只占一行入口");

// 在设置中配置模型服务与密钥
await card.getByRole("button", { name: "开启 AI 讲解" }).click();
const drawer = page.locator(".n-drawer");
await drawer.getByTestId("coach-enabled").waitFor();
check(await drawer.locator(".n-tabs-tab--active", { hasText: "AI 讲解" }).isVisible(), "从入口打开设置时直接显示 AI 讲解分组");
await drawer.getByPlaceholder("留空使用 https://api.deepseek.com").fill(LLM);
await drawer.getByPlaceholder("如 deepseek-chat").fill("mock-model");
await drawer.getByPlaceholder("填写新密钥").fill(SECRET);
await drawer.getByRole("button", { name: "保存", exact: true }).first().click();
await drawer.getByText("已配置").waitFor();
check(await drawer.getByPlaceholder("填写新密钥").inputValue() === "", "保存后输入框清空，密钥不回显");
await page.getByText("密钥已更新；其他修改尚未保存").waitFor();
check((await page.getByTestId("settings-state").innerText()) === "有未保存的修改", "密钥单独保存，其他修改仍提示未保存");
await page.getByTestId("coach-enabled").click();
await page.getByTestId("save-settings").click();
await page.getByText("设置已保存").waitFor();
check((await page.getByTestId("settings-state").innerText()) === "所有修改已保存", "保存后提示所有修改已保存");
await snap(page, "coach-settings");
await page.keyboard.press("Escape");
await drawer.waitFor({ state: "hidden" });

// 生成 AI 讲解
await page.getByTestId("coach-generate").waitFor({ timeout: 10000 });
check((await page.getByTestId("coach-status").innerText()).includes("入门"), "生成按钮旁显示学习水平与篇幅");
await page.getByTestId("coach-generate").click();
await page.getByTestId("coach-status").filter({ hasText: "已生成" }).waitFor({ timeout: 30000 });
const text = await card.innerText();
console.log(`  AI 讲解：${text.replace(/\n/g, " | ").slice(0, 200)}…`);
for (const t of ["为什么考虑这步", "为什么这样走", "学习要点"]) check(text.includes(t), `显示“${t}”段落`);
check(text.includes("引擎分析完整"), "分开显示分析状态");
check(text.includes("杀棋结论只出现在程序给出的"), "注明硬结论只由程序给出");
const basis = await page.getByTestId("coach-basis").allInnerTexts();
check(basis.length >= 2 && basis.every((b) => b.startsWith("依据：")), `每段下方显示程序给出的依据：${basis.join(" / ").slice(0, 80)}`);
check((await page.getByTestId("key-point").innerText()).length > 0, "一句话要点仍来自程序讲解");
await page.getByText("详细分析", { exact: true }).click();
check(await page.getByText("规则依据").isVisible(), "有 AI 讲解时，规则讲解的依据移到详细分析");

// 点击段落：只选中并高亮依据，不进入演示
const ctx = page.getByTestId("board-context");
const para = card.locator(".claims .claim").nth(2);
await para.click();
check(await para.evaluate((el) => el.classList.contains("active")), "点击段落后该段被选中");
check(!(await ctx.innerText()).includes("变化演示"), "点击段落不进入演示");
const marks = (await page.locator(".xq-board circle.hl").count()) > 0;
const elsewhere = await para.getByText("这条依据在落子前的局面上").isVisible().catch(() => false);
check(marks || elsewhere || (await para.innerText()).length > 0, `依据${marks ? "在棋盘上高亮" : elsewhere ? "在落子前局面，提示查看入口" : "没有可标出的棋子"}`);
// 独立的演示按钮
await card.getByRole("button", { name: "演示对方的应对" }).click();
await ctx.filter({ hasText: "变化演示" }).waitFor();
check(await page.getByRole("button", { name: "返回本步" }).isVisible(), "后续操作进入演示，并可返回本步");
await snap(page, "coach-ready");
await page.getByRole("button", { name: "返回本步" }).click();

// 模型请求与密钥
const calls = await (await fetch(`${LLM}/__calls`)).json();
check(calls.length === 1, `模型服务收到 ${calls.length} 次请求`);
check(calls[0].authorization === `Bearer ${SECRET}`, "请求带有后台保存的密钥");
check(calls[0].body.response_format?.type === "json_object", "DeepSeek 使用 JSON 输出模式");
check(!bodies.some((b) => b.includes(SECRET)), `${bodies.length} 个返回界面的响应中都没有密钥`);
check(!(await page.content()).includes(SECRET), "页面中没有密钥");

// 再次查看不重复调用；模型输出不合格时保留原结果
await page.locator(".root").click();
await page.locator(".move", { hasText: "炮二平五" }).click();
await page.getByTestId("coach-status").filter({ hasText: "已生成" }).waitFor();
check((await (await fetch(`${LLM}/__calls`)).json()).length === 1, "重新打开同一着复用结果，不再调用模型");
await fetch(`${LLM}/__mode`, { method: "POST", body: JSON.stringify({ mode: "bad" }) });
await card.getByRole("button", { name: "重新生成" }).click();
await card.getByText("最近一次重新生成失败").waitFor({ timeout: 30000 });
if (await card.getByRole("button", { name: "详情" }).isVisible()) await card.getByRole("button", { name: "详情" }).click();
const failed = await card.innerText();
check(failed.includes("没有通过检查"), "说明未通过检查的原因");
check((await page.getByTestId("coach-status").innerText()) === "已生成", "失败后状态仍是上一次的结果");
check(failed.includes("为什么这样走"), "失败后保留上一次的结果");
check((await (await fetch(`${LLM}/__calls`)).json()).length === 3, "不合格输出要求模型修正一次");
await snap(page, "coach-failed");

// 学习水平改变后，保留的结果不再对应当前设置；最近一次请求失败 → 显示“更新失败”的上次讲解，正文回到规则讲解
await page.getByRole("button", { name: "设置" }).click();
await drawer.locator(".n-tabs-tab", { hasText: "学习与显示" }).click();
await drawer.locator(".n-form-item", { hasText: "学习水平" }).locator(".n-base-selection").click();
await page.locator(".n-base-select-option", { hasText: "进阶" }).click();
await page.getByTestId("save-settings").click();
await page.getByTestId("settings-state").filter({ hasText: "所有修改已保存" }).waitFor();
await page.keyboard.press("Escape");
await drawer.waitFor({ state: "hidden" });
await page.getByTestId("coach-status").filter({ hasText: "更新失败" }).waitFor({ timeout: 10000 });
check(!(await card.innerText()).includes("已生成"), "保留的上次讲解不对应当前设置时，不显示“已生成”");
check(await page.getByTestId("coach-previous-note").isVisible(), "说明正在显示的是上次的讲解");
check(await page.locator(".panel h3", { hasText: "为什么这样走" }).isVisible(), "正文回到当前的规则讲解");
check((await card.locator(".claims").count()) === 0, "上次的讲解默认折叠");
const before = (await (await fetch(`${LLM}/__calls`)).json()).length;
await card.getByRole("button", { name: "重试" }).click();
await page.getByTestId("coach-status").filter({ hasText: "正在更新" }).waitFor({ timeout: 10000 }).catch(() => undefined);
await page.getByTestId("coach-status").filter({ hasText: "更新失败" }).waitFor({ timeout: 30000 });
check((await (await fetch(`${LLM}/__calls`)).json()).length > before, "重试后再次请求模型，仍然失败时继续注明是上次的讲解");
await page.getByTestId("coach-previous-toggle").click();
check((await card.locator(".faded .claims").count()) > 0, "展开后显示淡化的上次讲解");
await snap(page, "coach-previous");

// 旧版 AI 讲解的证据快照没有记录所在局面（on）：不画在棋盘上，并说明原因
await fetch(`${LLM}/__mode`, { method: "POST", body: JSON.stringify({ mode: "valid" }) });
const cfg = (await bridge("app_status")).config;
await bridge("update_config", { config: { ...cfg, coach: { ...cfg.coach, audience: "beginner" } } });
const strip = (view) => {
  // 新后台把早期快照中缺失的 on 读成 null
  for (const ev of view?.record?.result?.evidence ?? []) ev.on = null;
  return view;
};
await page.route("**/__bridge/invoke/node_detail", async (route) => {
  const body = await (await route.fetch()).json();
  strip(body.coach);
  await route.fulfill({ json: body });
});
await page.route("**/__bridge/invoke/coach_view", async (route) => {
  await route.fulfill({ json: strip(await (await route.fetch()).json()) });
});
await page.reload();
await page.waitForSelector(".xq-board");
await page.locator(".move", { hasText: "炮二平五" }).click();
await page.getByTestId("coach-status").filter({ hasText: "已生成" }).waitFor({ timeout: 10000 });
let legacyChecked = 0;
for (const claim of await card.locator(".claims .claim").all()) {
  await claim.click();
  const unknown = await claim.getByTestId("evidence-board-unknown").isVisible().catch(() => false);
  if (!unknown) continue;
  legacyChecked += 1;
  check((await page.locator(".xq-board circle.hl").count()) === 0, "没有记录局面的旧依据不画在棋盘上");
  check((await claim.getByRole("button", { name: "在落子前局面查看" }).count()) === 0, "也不提供落子前局面的查看入口");
}
check(legacyChecked > 0, `旧版依据说明了不在棋盘上标出的原因（${legacyChecked} 段）`);
await snap(page, "coach-legacy");

console.log(problems.length ? `控制台问题：\n${problems.join("\n")}` : "  ✓ 控制台无错误");
if (problems.length) process.exitCode = 1;
await browser.close();
