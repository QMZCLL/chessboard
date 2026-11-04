// 端到端场景的辅助函数：用无头 Chromium 操作真实界面（Vite + 开发桥 + 引擎）。
// 场景由 run.mjs 启动；单独运行某个场景时，需要自行设置 E2E_BASE 等环境变量（见 README.md）。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

export const BASE = process.env.E2E_BASE ?? "http://127.0.0.1:1420/";
/** 运行器的控制接口（重启开发桥等，见 run.mjs） */
export const CONTROL = process.env.E2E_CONTROL ?? null;
/** 模拟模型服务 */
export const LLM = process.env.MOCK_LLM ?? "http://127.0.0.1:1440";

// 场景脚本抛出异常（而不是检查没有通过）时以 2 退出，运行器据此把结果记为“运行出错”
for (const event of ["uncaughtException", "unhandledRejection"]) {
  process.on(event, (e) => {
    console.log(`  ✗ 运行出错：${e?.stack ?? e}`);
    process.exit(2);
  });
}
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

/**
 * 点棋盘上的一个格子（ICCS 坐标）。用真实的点击：格子要可见、没有被遮挡（例如对话框盖住主棋盘时不会点到下面的棋盘）。
 * scope 是页面或包含棋盘的区域（复习卡片、练习工作区）。
 */
export async function clickSquare(scope, sq) {
  await scope.locator(`[data-square="${sq}"]`).first().click();
}

export async function move(page, mv) {
  await clickSquare(page, mv.slice(0, 2));
  await clickSquare(page, mv.slice(2, 4));
}

export async function waitText(page, text, timeout = 30000) {
  await page.getByText(text, { exact: false }).first().waitFor({ timeout });
}

/**
 * 拦住页面发给后台的某个命令，用来安排请求与响应的先后；match 选出要拦的请求，其余照常发送。
 * 默认请求照常到达后台、只拦住响应；holdRequest 为 true 时连请求一起拦住，放行后才发给后台。
 * 返回 { arrived, release }：arrived 在请求拦住时完成（只拦响应时，后台已经处理完这个请求）；
 * release 放行，并等响应交给页面、界面处理完。
 */
export async function hold(page, cmd, { match = () => true, holdRequest = false } = {}) {
  const url = `**/__bridge/invoke/${cmd}`;
  let release;
  let arrive;
  let deliver;
  const released = new Promise((r) => (release = r));
  const arrived = new Promise((r) => (arrive = r));
  const delivered = new Promise((r) => (deliver = r));
  const handler = async (route) => {
    if (!match(route.request().postDataJSON())) return route.fallback();
    if (holdRequest) {
      arrive();
      await released;
    }
    const response = await route.fetch();
    arrive();
    await released;
    await route.fulfill({ response });
    deliver();
  };
  await page.route(url, handler);
  return {
    arrived,
    release: async () => {
      release();
      await delivered;
      await new Promise((r) => setTimeout(r, 300));
      await page.unroute(url, handler);
    },
  };
}

async function controlCall(route, body = {}) {
  if (!CONTROL) throw new Error("没有运行器的控制接口：这个场景要由 run.mjs 运行");
  const r = await fetch(`${CONTROL}${route}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const data = await r.json();
  if (!r.ok) throw new Error(`${route}：${data?.message}`);
  return data;
}

/**
 * 结束并重新启动开发桥（默认 SIGKILL，模拟断电或强制结束），等它重新就绪。
 * fresh 为 true 时换到新的空数据目录（模拟换了一台电脑）。返回 { data }：数据目录。
 */
export const restartBridge = ({ signal = "SIGKILL", fresh = false } = {}) => controlCall("/bridge/restart", { signal, fresh });

/** 等开发桥自己退出（在故障注入点终止），返回 { code, signal }；超时返回 null。 */
export const waitBridgeExit = (timeout_ms = 30000) => controlCall("/bridge/exited", { timeout_ms });

/** 当前开发桥的数据目录与模拟引擎程序。 */
export const bridgeInfo = () => controlCall("/bridge");

/** 设置开发桥中的故障注入点（action："abort" 终止进程、"error" 让这一步出错、null 清除）。 */
export const failpoint = (name, action) => bridge("dev_failpoint", { name, action });

/** 设置模拟模型服务的回复方式（见 mock-llm.mjs）。 */
export async function llmMode(body) {
  await fetch(`${LLM}/__mode`, { method: "POST", body: JSON.stringify(body) });
}

/** 模拟模型服务收到的请求。 */
export async function llmCalls() {
  return (await fetch(`${LLM}/__calls`)).json();
}

/** 等到条件成立（每 200 毫秒检查一次），超时时抛出异常。 */
export async function until(what, f, timeout = 30000) {
  const end = Date.now() + timeout;
  for (;;) {
    let v;
    try {
      v = await f();
    } catch (e) {
      if (Date.now() > end) throw e;
    }
    if (v) return v;
    if (Date.now() > end) throw new Error(`等待超时：${what}`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

/**
 * 保存的学习内容（经开发桥读出）：会话的棋谱树与讲解状态、复习卡片的作答与状态、学习事件。
 * 用来比较重启、崩溃或换数据目录前后是否相同。
 */
export async function stored() {
  const studies = [];
  for (const s of (await bridge("list_studies")).sort((a, b) => a.id.localeCompare(b.id))) {
    const v = await bridge("study_view", { id: s.id });
    const nodes = Object.values(v.study.nodes)
      .map((n) => ({ id: n.id, parent: n.parent, mv: n.mv, note: n.note, starred: n.starred, practice: n.practice?.sessions ?? 0 }))
      .sort((a, b) => a.id - b.id);
    studies.push({ id: s.id, name: s.name, nodes, explain: v.explain_status });
  }
  const cards = (await bridge("list_reviews"))
    .map((i) => ({ id: i.card.id, status: i.card.status, attempts: i.card.attempts.map((a) => a.at), due: i.card.due_at }))
    .sort((a, b) => a.id.localeCompare(b.id));
  const events = (await bridge("learning_events")).map((e) => e.id).sort();
  return { studies, cards, events };
}

/** 两份保存的内容相同时返回空字符串，否则说明第一处不同。 */
export function sameStored(a, b) {
  const sa = JSON.stringify(a);
  const sb = JSON.stringify(b);
  if (sa === sb) return "";
  for (const k of ["studies", "cards", "events"]) {
    if (JSON.stringify(a[k]) !== JSON.stringify(b[k])) return `${k} 不同：${JSON.stringify(a[k]).slice(0, 300)} ≠ ${JSON.stringify(b[k]).slice(0, 300)}`;
  }
  return "内容不同";
}

/** 页面上与开发桥连接中断有关的报错（重启开发桥期间预期会出现），其余的报错照常算问题。 */
export const transportNoise = (p) => /__bridge|net::ERR|Failed to load resource|\[http 50\d\]|ECONNREFUSED|请求失败|Failed to fetch/.test(p);

/** 读出诊断包（只存储、不压缩的 zip，见 xq-app 的 support::zip）：文件名 → 文字内容。 */
export function readZip(buf) {
  const files = {};
  let i = 0;
  while (i + 30 <= buf.length && buf.readUInt32LE(i) === 0x04034b50) {
    const size = buf.readUInt32LE(i + 18);
    const nameLen = buf.readUInt16LE(i + 26);
    const extra = buf.readUInt16LE(i + 28);
    const name = buf.subarray(i + 30, i + 30 + nameLen).toString("utf8");
    const start = i + 30 + nameLen + extra;
    files[name] = buf.subarray(start, start + size).toString("utf8");
    i = start + size;
  }
  return files;
}
