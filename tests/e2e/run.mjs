// 端到端场景运行器：构建开发桥与模拟引擎，启动 Vite、开发桥与模拟模型服务，按组运行场景，最后汇总结果。
// 每次运行使用临时数据目录与空闲端口，不影响本机正在运行的 pnpm dev / pnpm bridge。
//
// 用法：node tests/e2e/run.mjs [场景...] [--no-build] [--keep] [--repeat N] [--races]
//   场景可写全名（s7-coach）或编号（s7）；不写时运行全部。依赖其他场景数据的场景会自动先运行所依赖的场景。
//   --no-build  不重新构建开发桥与模拟引擎（已经构建过时）
//   --keep      保留临时数据目录（失败时总是保留）
//   --repeat N  连续运行 N 轮（每轮使用新的数据目录），汇总每个场景通过的轮数（发布前检查用）
//   --races     只运行竞态场景（RACES 中列出的）
//
// 开发桥带故障注入点构建（xq-devserver 的 failpoints 功能）。场景通过控制接口（环境变量 E2E_CONTROL）
// 强制结束或重启开发桥、换到新的数据目录（见 lib.mjs 的 restartBridge、waitBridgeExit）。
//
// 每个场景的结果分为：通过、断言失败（检查没有通过）、运行出错（场景脚本抛出异常）、超时、环境故障（服务没能启动）。
// 结果写在输出目录的 results.json 中；开发桥日志中出现 panic 时也算失败。
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
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
    scenarios: ["s1-record", "s2-endgame", "s3-capture", "s4-layout", "s5-escape", "s7-coach", "s9-workflow", "s10-review", "s11-reading", "s12-compare", "s13-hints", "s14-coach-pacing", "s15-review", "s16-coach-ask", "s17-review-decision", "s18-review-schedule", "s19-report-topics", "s20-practice-resume", "s22-learning-events", "s23-profile", "s26-inbox", "s27-revision-order"],
  },
  { name: "模拟引擎：不完整搜索与重试", engine: "engine-retry.txt", scenarios: ["s6-retry"] },
  { name: "模拟引擎：评分边界值", engine: "engine-bounds.txt", scenarios: ["s8-bounds"] },
  { name: "模拟引擎：排名仍在变化", engine: "engine-shifting.txt", scenarios: ["s21-stability"] },
  { name: "模拟引擎：从自己的棋局生成练习", engine: "engine-exercises.txt", scenarios: ["s24-exercises"] },
  { name: "模拟引擎：搜索较慢，会话视图较晚到达", engine: "engine-slow.txt", scenarios: ["s25-view-refresh"] },
  { name: "重启与恢复（自带 Pikafish）", engine: null, scenarios: ["s28-restart-recovery", "s29-crash-recovery", "s31-rapid-operations"] },
  { name: "外部依赖故障（模拟引擎）", engine: "engine-failures.txt", scenarios: ["s30-dependency-failures"] },
];
// 竞态与时序相关的场景（发布前检查中重复运行更多次）
const RACES = ["s15-review", "s16-coach-ask", "s19-report-topics", "s20-practice-resume", "s25-view-refresh", "s27-revision-order", "s31-rapid-operations"];
// 打开 s1 建立的“导入的棋谱”继续检查
const NEEDS = { "s2-endgame": ["s1-record"], "s4-layout": ["s1-record"] };
const SCENARIO_TIMEOUT_MS = 8 * 60 * 1000;

const args = process.argv.slice(2);
const repeatAt = args.indexOf("--repeat");
const repeat = repeatAt >= 0 ? Number(args.splice(repeatAt, 2)[1]) : 1;
if (!Number.isInteger(repeat) || repeat < 1) {
  console.error("--repeat 后面要写轮数");
  process.exit(2);
}
const flags = new Set(args.filter((a) => a.startsWith("--")));
const all = GROUPS.flatMap((g) => g.scenarios);
const wanted = new Set(flags.has("--races") ? RACES : []);
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
    [
      "build", "--manifest-path", path.join(server, "Cargo.toml"), "-p", "xq-devserver", "-p", "xq-app",
      "--bin", "xq-devserver", "--bin", "xq-mock-uci", "--features", "xq-devserver/failpoints",
    ],
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
function prepareData(group, index, round = 1, fresh = 0) {
  const dir = path.join(tmp, `round-${round}`, `group-${index + 1}${fresh ? `-fresh-${fresh}` : ""}`);
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

// ---------- 开发桥与控制接口 ----------
// 当前组的开发桥；场景可以通过控制接口重启它（同一个或新的数据目录）
const bridgeState = { child: null, data: null, group: null, index: 0, round: 1, starts: 0, fresh: 0, logs: [] };

async function startBridge(data) {
  bridgeState.starts += 1;
  const log = `bridge-r${bridgeState.round}-g${bridgeState.index + 1}-${bridgeState.starts}`;
  bridgeState.logs.push(path.join(out, "logs", `${log}.log`));
  const child = startProcess(log, devserver, ["--port", String(bridgePort), "--data", data], {
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? "info" },
  });
  bridgeState.child = child;
  bridgeState.data = data;
  // 引擎可能是故意不可用的（外部依赖故障场景）：开发桥能回答即可，正常的组再等引擎就绪
  await waitFor("开发桥", async () => !!(await bridgeStatus()), 60000, child);
  if (bridgeState.group?.engine !== "engine-failures.txt") {
    await waitFor("开发桥与引擎", async () => (await bridgeStatus())?.engine?.state === "ready", 120000, child);
  }
}

function exited(child, timeoutMs) {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) return resolve({ code: child.exitCode, signal: child.signalCode });
    const timer = setTimeout(() => resolve(null), timeoutMs);
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal });
    });
  });
}

const controlPort = await freePort();
const control = http.createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", async () => {
    const reply = (status, value) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(value));
    };
    try {
      const args = body ? JSON.parse(body) : {};
      if (req.url === "/bridge") return reply(200, { data: bridgeState.data, pid: bridgeState.child?.pid ?? null, mockEngine });
      if (req.url === "/bridge/exited") {
        // 等开发桥自己退出（故障注入点终止进程），返回退出状态；超时返回 null
        return reply(200, await exited(bridgeState.child, args.timeout_ms ?? 30000));
      }
      if (req.url === "/bridge/restart") {
        // 结束开发桥（默认 SIGKILL：模拟断电或强制结束）并重新启动；fresh 为 true 时换到新的空数据目录
        const old = bridgeState.child;
        if (old && old.exitCode === null && old.signalCode === null) {
          old.kill(args.signal ?? "SIGKILL");
          await exited(old, 30000);
        }
        let data = bridgeState.data;
        if (args.fresh) {
          bridgeState.fresh += 1;
          data = prepareData(bridgeState.group, bridgeState.index, bridgeState.round, bridgeState.fresh);
        }
        await startBridge(data);
        return reply(200, { data });
      }
      reply(404, { message: `没有 ${req.url}` });
    } catch (e) {
      reply(500, { message: e.message });
    }
  });
});
await new Promise((r) => control.listen(controlPort, "127.0.0.1", r));

/** 开发桥日志中的 panic（不论是否被捕获，都说明有未处理的错误）。 */
function panicsIn(files) {
  const found = [];
  for (const f of files) {
    const text = fs.existsSync(f) ? fs.readFileSync(f, "utf8") : "";
    for (const line of text.split("\n")) if (line.includes("panicked at")) found.push(`${path.basename(f)}：${line.trim()}`);
  }
  return found;
}

const STATUS_TEXT = {
  passed: "通过",
  assertion_failed: "断言失败",
  operation_failed: "运行出错",
  timed_out: "超时",
  infrastructure_failed: "环境故障",
};

function runScenario(name, round) {
  return new Promise((resolve) => {
    console.log(`\n== ${name}${repeat > 1 ? `（第 ${round} 轮）` : ""}`);
    const started = Date.now();
    let timedOut = false;
    const child = spawn(process.execPath, [path.join(here, "scenarios", `${name}.mjs`)], {
      cwd: here,
      stdio: "inherit",
      env: {
        ...process.env,
        E2E_BASE: base,
        E2E_OUT: out,
        MOCK_LLM: `http://127.0.0.1:${llmPort}`,
        E2E_CONTROL: `http://127.0.0.1:${controlPort}`,
        E2E_MOCK_ENGINE: mockEngine,
      },
    });
    const timer = setTimeout(() => {
      timedOut = true;
      console.log(`  ✗ ${name} 超过 ${SCENARIO_TIMEOUT_MS / 60000} 分钟，已停止`);
      child.kill();
    }, SCENARIO_TIMEOUT_MS);
    child.on("exit", (code, signal) => {
      clearTimeout(timer);
      // 场景的退出码：0 通过，1 有检查没有通过，2 脚本抛出异常（见 lib.mjs）
      const status = timedOut ? "timed_out" : code === 0 ? "passed" : code === 1 ? "assertion_failed" : "operation_failed";
      resolve({ name, round, status, ok: status === "passed", code: code ?? signal, seconds: Math.round((Date.now() - started) / 1000) });
    });
  });
}

// ---------- 按组运行 ----------
const results = [];
const panics = [];
for (let round = 1; round <= repeat; round++) {
  for (const [i, group] of GROUPS.entries()) {
    const scenarios = group.scenarios.filter((s) => wanted.has(s));
    if (!scenarios.length) continue;
    console.log(`\n### ${group.name}${repeat > 1 ? `（第 ${round} 轮）` : ""}`);
    Object.assign(bridgeState, { group, index: i, round, starts: 0, fresh: 0, logs: [] });
    try {
      await startBridge(prepareData(group, i, round));
    } catch (e) {
      console.error(`\n环境故障：${e.message}`);
      for (const s of scenarios) results.push({ name: s, round, status: "infrastructure_failed", ok: false, code: "error", seconds: 0 });
      if (bridgeState.child) await stopProcess(bridgeState.child);
      continue;
    }
    for (const s of scenarios) results.push(await runScenario(s, round));
    await stopProcess(bridgeState.child);
    panics.push(...panicsIn(bridgeState.logs));
  }
}
control.close();
await Promise.all([...children].map(stopProcess));

// ---------- 汇总 ----------
fs.writeFileSync(path.join(out, "results.json"), JSON.stringify({ repeat, results, panics }, null, 2));
console.log("\n场景结果：");
if (repeat === 1) {
  for (const r of results) console.log(`  ${r.ok ? "✓" : "✗"} ${r.name}（${r.seconds} 秒${r.ok ? "" : `，${STATUS_TEXT[r.status]}，退出状态 ${r.code}`}）`);
} else {
  const names = [...new Set(results.map((r) => r.name))];
  for (const n of names) {
    const runs = results.filter((r) => r.name === n);
    const bad = runs.filter((r) => !r.ok);
    const kinds = [...new Set(bad.map((r) => STATUS_TEXT[r.status]))].join("、");
    console.log(`  ${bad.length ? "✗" : "✓"} ${n}：${runs.length - bad.length}/${runs.length} 轮通过${bad.length ? `（${kinds}，第 ${bad.map((r) => r.round).join("、")} 轮）` : ""}`);
  }
}
if (panics.length) {
  console.log("\n开发桥日志中出现 panic：");
  for (const p of panics.slice(0, 20)) console.log(`  ${p}`);
}
const failed = results.filter((r) => !r.ok);
console.log(`截图与日志：${out}（结果：results.json）`);
if (failed.length || panics.length || flags.has("--keep")) console.log(`数据目录：${tmp}`);
else fs.rmSync(tmp, { recursive: true, force: true });
process.exit(failed.length || panics.length ? 1 : 0);
