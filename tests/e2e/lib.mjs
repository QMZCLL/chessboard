// 端到端场景的辅助函数：用无头 Chromium 操作真实界面（Vite + 开发桥 + 引擎）。
// 场景由 run.mjs 启动；单独运行某个场景时，需要自行设置 E2E_BASE 等环境变量（见 README.md）。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

export const BASE = process.env.E2E_BASE ?? "http://127.0.0.1:1420/";
/** 截图目录 */
export const OUT = process.env.E2E_OUT ?? path.join(path.dirname(fileURLToPath(import.meta.url)), ".out");
fs.mkdirSync(OUT, { recursive: true });

export async function launch({ width = 1440, height = 900, dark = false } = {}) {
  const browser = await chromium.launch({
    // 不设置时使用 playwright-core 安装的 Chromium（pnpm exec playwright-core install chromium）
    executablePath: process.env.E2E_CHROMIUM || undefined,
    headless: true,
  });
  const context = await browser.newContext({ viewport: { width, height }, colorScheme: dark ? "dark" : "light", locale: "zh-CN" });
  const page = await context.newPage();
  const problems = [];
  page.on("console", (m) => {
    if (m.type() === "error" || m.type() === "warning") problems.push(`[${m.type()}] ${m.text()}`);
  });
  page.on("pageerror", (e) => problems.push(`[pageerror] ${e.message}`));
  page.on("response", (r) => {
    if (r.url().includes("/__bridge/invoke/") && r.status() >= 400) problems.push(`[http ${r.status()}] ${r.url()}`);
  });
  return { browser, context, page, problems };
}

const scenario = path.basename(process.argv[1] ?? "scenario", ".mjs");
let shot = 0;
/** 截图，文件名带场景名与序号。 */
export async function snap(page, name) {
  shot += 1;
  const file = path.join(OUT, `${scenario}-${String(shot).padStart(2, "0")}-${name}.png`);
  await page.screenshot({ path: file });
  console.log(`  📸 ${file}`);
  return file;
}

/** 记录一项检查；失败时场景以非零状态退出，但继续执行后面的检查。 */
export function check(cond, msg) {
  if (!cond) {
    console.log(`  ✗ ${msg}`);
    process.exitCode = 1;
  } else console.log(`  ✓ ${msg}`);
}

/** 直接调用开发桥命令（经 Vite 代理），用于准备数据与核对后台状态。 */
export async function bridge(cmd, args = {}) {
  const r = await fetch(`${BASE}__bridge/invoke/${cmd}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(args) });
  const data = await r.json().catch(() => null);
  if (!r.ok) throw new Error(`${cmd}: ${data?.message}`);
  return data;
}

/** 点击棋盘交叉点（ICCS 坐标）。 */
export async function clickSquare(page, sq) {
  await page.locator(`[data-square="${sq}"]`).first().dispatchEvent("mousedown", { button: 0 });
}

export async function move(page, mv) {
  await clickSquare(page, mv.slice(0, 2));
  await clickSquare(page, mv.slice(2, 4));
}

export async function waitText(page, text, timeout = 30000) {
  await page.getByText(text, { exact: false }).first().waitFor({ timeout });
}
