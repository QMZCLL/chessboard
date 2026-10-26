// 端到端场景运行器：构建开发桥与模拟引擎，启动 Vite、开发桥与模拟模型服务，按组运行场景，最后汇总结果。
// 每次运行使用临时数据目录与空闲端口，不影响本机正在运行的 pnpm dev / pnpm bridge。
//
// 用法：node tests/e2e/run.mjs [场景...] [--no-build] [--keep]
//   场景可写全名（s7-coach）或编号（s7）；不写时运行全部。依赖其他场景数据的场景会自动先运行所依赖的场景。
//   --no-build  不重新构建开发桥与模拟引擎（已经构建过时）
//   --keep      保留临时数据目录（失败时总是保留）
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../..");
const server = path.join(repo, "server");
const exe = process.platform === "win32" ? ".exe" : "";

// 同一组的场景共用一个开发桥（同一个数据目录），按列出的顺序运行
const GROUPS = [
  {
    name: "仓库自带的 Pikafish",
    engine: null,
    scenarios: ["s1-record", "s2-endgame", "s3-capture", "s4-layout", "s5-escape", "s7-coach", "s9-workflow", "s10-review", "s11-reading", "s12-compare", "s13-hints", "s14-coach-pacing", "s15-review", "s16-coach-ask", "s17-review-decision", "s18-review-schedule"],
  },
  { name: "模拟引擎：不完整搜索与重试", engine: "engine-retry.txt", scenarios: ["s6-retry"] },
  { name: "模拟引擎：评分边界值", engine: "engine-bounds.txt", scenarios: ["s8-bounds"] },
];
// 打开 s1 建立的“导入的棋谱”继续检查
const NEEDS = { "s2-endgame": ["s1-record"], "s4-layout": ["s1-record"] };
const SCENARIO_TIMEOUT_MS = 8 * 60 * 1000;

const args = process.argv.slice(2);
const flags = new Set(args.filter((a) => a.startsWith("--")));
const all = GROUPS.flatMap((g) => g.scenarios);
const wanted = new Set();
for (const a of args.filter((x) => !x.startsWith("--"))) {
  const name = all.find((s) => s === a || s.startsWith(`${a}-`));
  if (!name) {
    console.error(`没有场景 ${a}。可用的场景：${all.join(" ")}`);
    process.exit(2);
  }
  wanted.add(name);
}
if (wanted.size === 0) all.forEach((s) => wanted.add(s));
for (const s of [...wanted]) for (const dep of NEEDS[s] ?? []) wanted.add(dep);

const out = path.resolve(process.env.E2E_OUT ?? path.join(here, ".out"));
// 只清理上一次运行留下的截图与日志，不删除目录中的其他文件
fs.mkdirSync(out, { recursive: true });
for (const f of fs.readdirSync(out)) if (f.endsWith(".png")) fs.rmSync(path.join(out, f));
fs.rmSync(path.join(out, "logs"), { recursive: true, force: true });
fs.mkdirSync(path.join(out, "logs"), { recursive: true });
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "xq-e2e-"));

// ---------- 子进程 ----------
const children = new Set();
function startProcess(name, cmd, cmdArgs, opts = {}) {
  const log = fs.openSync(path.join(out, "logs", `${name}.log`), "w");
  const child = spawn(cmd, cmdArgs, { ...opts, stdio: ["ignore", log, log] });
  children.add(child);
  child.on("exit", () => children.delete(child));
  return child;
}
function stopProcess(child) {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) return resolve();
    child.once("exit", () => resolve());
    child.kill();
  });
}
function stopAll() {
  for (const c of children) c.kill();
}
process.on("exit", stopAll);
for (const sig of ["SIGINT", "SIGTERM"]) {
  process.on(sig, () => {
    stopAll();
    process.exit(130);
  });
}

function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

async function waitFor(what, probe, timeoutMs, child) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child && child.exitCode !== null) throw new Error(`${what}：进程已退出（见 ${path.join(out, "logs")}）`);
    try {
      if (await probe()) return;
    } catch {
      // 尚未就绪
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`${what}：等待超时（见 ${path.join(out, "logs")}）`);
}

// ---------- 构建 ----------
const cargo = process.env.CARGO ?? "cargo";
const target = process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(server, "target");
const devserver = path.join(target, "debug", `xq-devserver${exe}`);
const mockEngine = path.join(target, "debug", `xq-mock-uci${exe}`);
if (!flags.has("--no-build")) {
  console.log("构建开发桥与模拟引擎…");
  const r = spawnSync(
    cargo,
    ["build", "--manifest-path", path.join(server, "Cargo.toml"), "-p", "xq-devserver", "-p", "xq-app", "--bin", "xq-devserver", "--bin", "xq-mock-uci"],
    { stdio: "inherit" },
  );
  if (r.status !== 0) process.exit(r.status ?? 1);
}
for (const bin of [devserver, mockEngine]) {
  if (!fs.existsSync(bin)) {
    console.error(`找不到 ${bin}。去掉 --no-build 重新运行，或设置 CARGO_TARGET_DIR 指向构建目录。`);
    process.exit(1);
  }
}

// ---------- 服务 ----------
const [vitePort, bridgePort, llmPort] = [await freePort(), await freePort(), await freePort()];
const base = `http://127.0.0.1:${vitePort}/`;

const llm = startProcess("mock-llm", process.execPath, [path.join(here, "mock-llm.mjs")], {
  env: { ...process.env, MOCK_LLM_PORT: String(llmPort) },
});
await waitFor("模拟模型服务", async () => (await fetch(`http://127.0.0.1:${llmPort}/__calls`)).ok, 15000, llm);

const vite = startProcess(
  "vite",
  process.execPath,
  [path.join(repo, "node_modules", "vite", "bin", "vite.js"), "--port", String(vitePort), "--strictPort", "--host", "127.0.0.1"],
  { cwd: repo, env: { ...process.env, XQ_BRIDGE_PORT: String(bridgePort) } },
);
await waitFor("Vite", async () => (await fetch(base)).ok, 60000, vite);

/**
 * 为一组场景准备数据目录与引擎配置。
 * 自带 Pikafish：单线程、按固定深度搜索（时间上限放宽），每次搜索都完成整轮，结果不受机器快慢影响；
 * 默认的按时间搜索在较慢或繁忙的机器上可能在一轮中途停止，讲解会如实标为“分析不完整”，使场景结果不稳定。
 * 模拟引擎：写入引擎程序与脚本。
 */
function prepareData(group, index) {
  const dir = path.join(tmp, `group-${index + 1}`);
  fs.mkdirSync(path.join(dir, "config"), { recursive: true });
  if (!group.engine) {
    const config = {
      version: 1,
      engine: { threads: 1, hash_mb: 64 },
      analysis: { quick_time_ms: 10000, quick_depth: 12, deep_time_ms: 20000, deep_depth: 16 },
    };
    fs.writeFileSync(path.join(dir, "config", "config.json"), JSON.stringify(config, null, 2));
  } else {
    const engineDir = path.join(dir, "engine");
    fs.mkdirSync(engineDir, { recursive: true });
    const binary = path.join(engineDir, path.basename(mockEngine));
    fs.copyFileSync(mockEngine, binary);
    fs.chmodSync(binary, 0o755);
    // 模拟引擎从工作目录（引擎所在目录）读取脚本
    fs.copyFileSync(path.join(here, "fixtures", group.engine), path.join(engineDir, "mock-uci-script.txt"));
    const config = { version: 1, engine: { binary, nnue: null, threads: 1, hash_mb: 16 }, analysis: { multipv: 2 } };
    fs.writeFileSync(path.join(dir, "config", "config.json"), JSON.stringify(config, null, 2));
  }
  return dir;
}

async function bridgeStatus() {
  const r = await fetch(`http://127.0.0.1:${bridgePort}/invoke/app_status`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: "{}",
  });
  return r.ok ? r.json() : null;
}

function runScenario(name) {
  return new Promise((resolve) => {
    console.log(`\n== ${name}`);
    const started = Date.now();
    const child = spawn(process.execPath, [path.join(here, "scenarios", `${name}.mjs`)], {
      cwd: here,
      stdio: "inherit",
      env: { ...process.env, E2E_BASE: base, E2E_OUT: out, MOCK_LLM: `http://127.0.0.1:${llmPort}` },
    });
    const timer = setTimeout(() => {
      console.log(`  ✗ ${name} 超过 ${SCENARIO_TIMEOUT_MS / 60000} 分钟，已停止`);
      child.kill();
    }, SCENARIO_TIMEOUT_MS);
    child.on("exit", (code, signal) => {
      clearTimeout(timer);
      resolve({ name, ok: code === 0, code: code ?? signal, seconds: Math.round((Date.now() - started) / 1000) });
    });
  });
}

// ---------- 按组运行 ----------
const results = [];
try {
  for (const [i, group] of GROUPS.entries()) {
    const scenarios = group.scenarios.filter((s) => wanted.has(s));
    if (!scenarios.length) continue;
    console.log(`\n### ${group.name}`);
    const data = prepareData(group, i);
    const bridge = startProcess(`bridge-${i + 1}`, devserver, ["--port", String(bridgePort), "--data", data], {
      env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? "info" },
    });
    await waitFor("开发桥与引擎", async () => (await bridgeStatus())?.engine?.state === "ready", 120000, bridge);
    for (const s of scenarios) results.push(await runScenario(s));
    await stopProcess(bridge);
  }
} catch (e) {
  console.error(`\n运行中断：${e.message}`);
  results.push({ name: "（环境）", ok: false, code: "error", seconds: 0 });
}
await Promise.all([...children].map(stopProcess));

// ---------- 汇总 ----------
console.log("\n场景结果：");
for (const r of results) console.log(`  ${r.ok ? "✓" : "✗"} ${r.name}（${r.seconds} 秒${r.ok ? "" : `，退出状态 ${r.code}`}）`);
const failed = results.filter((r) => !r.ok);
console.log(`截图与日志：${out}`);
if (failed.length || flags.has("--keep")) console.log(`数据目录：${tmp}`);
else fs.rmSync(tmp, { recursive: true, force: true });
process.exit(failed.length ? 1 : 0);
