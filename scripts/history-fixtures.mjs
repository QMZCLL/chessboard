#!/usr/bin/env node
// 用旧版本的代码生成历史数据样本（tests/history/ 下的样本由它生成，生成后不再修改）。
//
// 用法：node scripts/history-fixtures.mjs --devserver <旧版本的 xq-devserver> --mock-llm <旧版本的 mock-llm.mjs>
//         --profile v0.3.0|ab20742 --out <样本目录> [--interrupted]
//
// 脚本启动旧版本的开发桥（使用旧版本仓库中的 Pikafish，单线程、固定深度），通过它的命令接口建立会话、讲解、
// 收藏与笔记、分支与删除、先想后讲、复习与局面决策、出题、一组没有做完的练习、AI 讲解与追问、原会话已删除的卡片，
// 再导出学习档案与单个会话、立即备份，并用旧版本自己的接口读出全部内容（old-api.json），作为与新版本比较的依据。
// --interrupted 时在 AI 讲解、追问、局面决策与深入分析都还在进行时直接结束进程，留下中断的状态。
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";

const args = new Map();
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a === "--interrupted") args.set("interrupted", true);
  else args.set(a.replace(/^--/, ""), process.argv[++i]);
}
for (const k of ["devserver", "mock-llm", "profile", "out"]) {
  if (!args.get(k)) throw new Error(`缺少参数 --${k}`);
}
const profile = args.get("profile");
const interrupted = Boolean(args.get("interrupted"));
const out = path.resolve(args.get("out"));
if (fs.existsSync(out)) throw new Error(`${out} 已存在：样本生成后不再修改，请换一个目录`);
const data = path.join(out, "home");
fs.mkdirSync(path.join(data, "config"), { recursive: true });

async function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

const [bridgePort, llmPort] = [await freePort(), await freePort()];
const logs = path.join(process.env.FIXTURE_LOGS ?? path.join(out, "..", ".logs"), path.basename(out));
fs.mkdirSync(logs, { recursive: true });
function start(name, cmd, cmdArgs, env = {}) {
  const log = fs.openSync(path.join(logs, `${name}.log`), "w");
  return spawn(cmd, cmdArgs, { stdio: ["ignore", log, log], env: { ...process.env, ...env } });
}

// 单线程、固定深度：每次搜索都完成整轮；AI 讲解使用本机的模拟模型服务
const config = {
  version: 1,
  engine: { threads: 1, hash_mb: 32 },
  analysis: { quick_time_ms: 20000, quick_depth: 8, deep_time_ms: 30000, deep_depth: 10, multipv: 3 },
};
fs.writeFileSync(path.join(data, "config", "config.json"), JSON.stringify(config, null, 2));

const llm = start("mock-llm", process.execPath, [args.get("mock-llm")], { MOCK_LLM_PORT: String(llmPort) });
const bridgeProc = start("bridge", args.get("devserver"), ["--port", String(bridgePort), "--data", data]);
const stopAll = () => {
  for (const c of [bridgeProc, llm]) if (c.exitCode === null) c.kill("SIGKILL");
};
process.on("exit", stopAll);

async function call(cmd, body = {}) {
  const r = await fetch(`http://127.0.0.1:${bridgePort}/invoke/${cmd}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const v = await r.json().catch(() => null);
  if (!r.ok) throw new Error(`${cmd}: ${v?.message ?? r.status}`);
  return v;
}

async function until(what, f, timeout = 180000) {
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

const llmMode = (body) =>
  fetch(`http://127.0.0.1:${llmPort}/__mode`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

await until("开发桥与引擎", async () => (await call("app_status"))?.engine?.state === "ready");
await until("模拟模型服务", async () => (await fetch(`http://127.0.0.1:${llmPort}/__calls`)).ok);

const idle = (id) => until(`会话 ${id} 的讲解完成`, async () => (await call("study_view", { id })).stats.pending === 0);
const mainLine = (study) => {
  const line = [];
  let cur = study.nodes[String(study.root ?? 0)] ?? study.nodes["0"];
  while (cur && cur.children.length > 0) {
    cur = study.nodes[String(cur.children[0])];
    line.push(cur);
  }
  return line;
};
const legal = async (study_id, node_id) => (await call("node_detail", { req: { study_id, node_id } })).legal_moves;

let status = await call("app_status");
await call("update_config", {
  config: {
    ...status.config,
    coach: { ...status.config.coach, enabled: true, base_url: `http://127.0.0.1:${llmPort}`, model: "mock-model" },
    ...(profile === "ab20742" ? { player: { names: ["样本棋手"] } } : {}),
  },
});
// 样本中的密钥是虚构的
await call("set_coach_credential", { req: { key: "sk-fixture-not-a-secret" } });

// 会话 A：导入的对局（中炮对屏风马，第 5 回合红方送车）
const RECORD_A = [
  '[Red "样本棋手"]',
  '[Black "对手甲"]',
  '[Date "2026.10.01"]',
  "1. 炮二平五 马8进7 2. 马二进三 车9平8 3. 车一平二 马2进3 4. 兵七进一 卒7进1 5. 车二进六 炮8平9 6. 车二平三 炮9退1",
].join("\n");
const imported = await call("import_text", { text: RECORD_A, name: "样本：中炮对屏风马" });
const A = imported.study.study.id;
await idle(A);
let a = (await call("study_view", { id: A })).study;
let line = mainLine(a);

await call("set_note", { req: { study_id: A, node_id: line[2].id, note: "这里要注意中路", starred: true } });
// 分支：第 2 着换一种走法再续一着；之后删除另一条分支（编号不再使用）
const alt = (await legal(A, line[0].id)).find((m) => m !== line[1].mv);
await call("add_line", { req: { study_id: A, parent: line[0].id, moves: [alt] } });
const extra = (await legal(A, line[1].id)).find((m) => m !== line[2].mv);
const doomed = await call("add_line", { req: { study_id: A, parent: line[1].id, moves: [extra] } });
await call("delete_node", { req: { study_id: A, node_id: doomed.node_id } });
await idle(A);
a = (await call("study_view", { id: A })).study;
line = mainLine(a);

// 先想后讲：看了一级提示，第一次选的不是棋谱着法
const wrong4 = (await legal(A, line[2].id)).find((m) => m !== line[3].mv);
await call("record_practice", {
  req: { study_id: A, node_id: line[3].id, hints_used: 1, revealed_directly: false, first_choice: wrong4, hints: null },
});
const card1 = (await call("add_review", { req: { study_id: A, node_id: line[3].id } })).card.id;
const card2 = (await call("add_review", { req: { study_id: A, node_id: line[5].id } })).card.id;
await call("review_attempt", {
  req: { id: card1, choice: wrong4, goal: "recall", intent: "develop", practice: null, request: randomUUID(), hints: 1, elapsed_ms: 5200, confidence: "unsure" },
});
await call("update_review", { req: { id: card1, status: "again", note: "没看到对方的炮", issues: ["missed_threat"] } });
await call("review_attempt", {
  req: { id: card2, choice: line[5].mv, goal: "decision", intent: "capture", practice: null, request: randomUUID(), hints: 0, elapsed_ms: 8100, confidence: "sure" },
});
const decisionDone = async (id) => {
  const d = await call("review_detail", { req: { id } });
  return JSON.stringify(d).match(/"state":"(running|queued|pending)"/) ? null : d;
};
if (!interrupted) await until("局面决策的比较", () => decisionDone(card2));

await call("update_study", { req: { study_id: A, user_side: "red" } });
const exercises = await call("generate_exercises", { req: { study_id: A } });
console.log(`出题：${exercises.text}`);

// AI 讲解与追问
if (interrupted) await llmMode({ mode: "valid", delay_ms: 600000 });
await call("coach_explain", { req: { study_id: A, node_id: line[1].id, refresh: false } });
if (!interrupted) {
  await until("AI 讲解", async () => (await call("coach_view", { req: { study_id: A, node_id: line[1].id } })).status === "ready");
}
const mine = (await legal(A, line[3].id)).find((m) => m !== line[4].mv);
const cmp = await call("compare_moves", { req: { study_id: A, node_id: line[3].id, mine, reference: line[4].mv } });
if (!interrupted) {
  await until("对比", async () => {
    const v = await call("compare_moves", { req: { study_id: A, node_id: line[3].id, mine, reference: line[4].mv } });
    return !v.pending && v;
  });
}
const question = { study_id: A, node_id: line[3].id, mine, reference: line[4].mv, kind: "difference" };
await call("ask_coach", { req: question });
if (!interrupted) {
  await until("追问的回答", async () => {
    const answers = await call("coach_answers", { req: { study_id: A, node_id: line[3].id, mine, reference: line[4].mv } });
    return answers.some((x) => x.status === "ready") && answers;
  });
}
void cmp;

// 会话 B：残局（执红、正被将军）；先想后讲没有选对，出一道找威胁；加入复习后删除会话，卡片保留
const B = (
  await call("create_study", {
    req: { name: "样本：马炮残局", kind: { type: "fen", fen: "3k5/9/4c4/9/9/9/9/4N4/9/4K4 w", side: null }, orientation: null, user_side: "red" },
  })
).study.study.id;
const bWrong = (await legal(B, 0)).find((m) => m !== "e2c3");
const bMove = await call("apply_move", { req: { study_id: B, parent: 0, mv: "e2c3", variation: false } });
await idle(B);
await call("record_practice", { req: { study_id: B, node_id: bMove.node_id, hints_used: 0, revealed_directly: false, first_choice: bWrong, hints: null } });
console.log(`残局出题：${(await call("generate_exercises", { req: { study_id: B } })).text}`);
await call("add_review", { req: { study_id: B, node_id: bMove.node_id } });
await call("delete_study", { id: B });

// 一组没有做完的练习：答了第一题，停在第二题
const cards = (await call("list_reviews")).map((c) => c.card.id);
const session = await call("start_practice", { req: { label: "今日练习", source: "today", topic: null, goal: "recall", cards: cards.slice(0, 3) } });
const first = await call("review_detail", { req: { id: session.cards[0] } });
const firstChoice = first.card.answer.mv;
await call("review_attempt", {
  req: { id: session.cards[0], choice: firstChoice, goal: "recall", intent: null, practice: session.id, request: randomUUID(), hints: 0, elapsed_ms: 3000, confidence: "guess" },
}).catch((e) => console.log(`练习第一题作答：${e.message}`));
await call("update_practice", { req: { id: session.id, index: 1, skip: null, goal: null, finish: false } });

// 会话 C：标准开局，执黑、棋盘翻转，走了两着
const C = (
  await call("create_study", { req: { name: "样本：执黑练习", kind: { type: "standard" }, orientation: "black_bottom", user_side: "black" } })
).study.study.id;
const c1 = await call("apply_move", { req: { study_id: C, parent: 0, mv: "h2e2", variation: false } });
await call("apply_move", { req: { study_id: C, parent: c1.node_id, mv: "h9g7", variation: false } });
await call("update_study", { req: { study_id: A, name: "样本：中炮对屏风马（复盘）" } });

// 0.4 起：复盘收件箱
if (profile === "ab20742") {
  const games = [
    '[Red "对手乙"]\n[Black "样本棋手"]\n[Date "2026.10.02"]\n1. 相三进五 马8进7 2. 马二进三 卒7进1 3. 兵七进一 马2进3',
    '[Red "样本棋手"]\n[Black "对手丙"]\n[Date "2026.10.03"]\n1. 兵七进一 卒7进1 2. 马八进七 马8进7 3. 炮二平三 象7进5',
  ].join("\n\n");
  const preview = await call("inbox_preview", { req: { sources: [{ kind: "text", name: "样本收件箱.pgn", text: games }] } });
  const picks = preview.games.map((g) => ({ text: g.text, name: g.name ?? null, user_side: g.user_side ?? "red" }));
  await call("inbox_import", { req: { games: picks } });
  if (!interrupted) {
    await until("收件箱分析", async () => {
      const list = await call("inbox_list");
      return list.items.every((i) => i.done) && list;
    }, 300000).catch(async () => console.log(`收件箱：${JSON.stringify(await call("inbox_list")).slice(0, 400)}`));
    console.log(`收件箱出题：${JSON.stringify(await call("inbox_generate")).slice(0, 200)}`);
  }
}

if (interrupted) {
  // AI 讲解（模型服务延迟回复）、深入分析与另一张卡片的局面决策都在进行时结束进程：
  // 先把搜索深度调大，让这些搜索在结束进程时还没有完成
  status = await call("app_status");
  await call("update_config", {
    config: { ...status.config, analysis: { ...status.config.analysis, quick_depth: 30, quick_time_ms: 120000, deep_depth: 40, deep_time_ms: 120000 } },
  });
  await call("explain_move", { req: { study_id: A, node_id: line[6].id, budget: "deep", retry: false } });
  await call("explain_move", { req: { study_id: A, node_id: line[7].id, budget: "deep", retry: false } });
  await call("review_attempt", {
    req: { id: card1, choice: line[3].mv, goal: "decision", intent: "develop", practice: null, request: randomUUID(), hints: 0, elapsed_ms: 4000, confidence: "sure" },
  });
  await new Promise((r) => setTimeout(r, 1500));
  bridgeProc.kill("SIGKILL");
  await new Promise((r) => bridgeProc.once("exit", r));
  console.log("已在任务进行中结束开发桥");
} else {
  // 等全部后台讲解（包括补充的深入分析）结束：连续三次读到所有会话都没有进行中的讲解
  let quiet = 0;
  await until("全部后台讲解结束", async () => {
    const studies = await call("list_studies");
    const views = await Promise.all(studies.map((s) => call("study_view", { id: s.id })));
    quiet = views.every((v) => v.stats.pending === 0) ? quiet + 1 : 0;
    await new Promise((r) => setTimeout(r, 1000));
    return quiet >= 3;
  }, 600000);
  // 旧版本自己的导出与读出的内容
  fs.writeFileSync(path.join(out, "archive.json"), await call("export_archive"));
  fs.writeFileSync(path.join(out, "study-export.json"), await call("export_study", { id: A }));
  await call("backup_now");
  const dump = { profile, studies: {}, reviews: {}, coach: {}, answers: [] };
  // 只保留版本与设置：状态中的数据目录与日志目录是本机路径
  const { version, config: savedConfig } = await call("app_status");
  dump.status = { version, config: savedConfig };
  dump.list_studies = await call("list_studies");
  for (const s of dump.list_studies) {
    dump.studies[s.id] = await call("study_view", { id: s.id });
  }
  dump.list_reviews = await call("list_reviews");
  for (const c of dump.list_reviews) dump.reviews[c.card.id] = await call("review_detail", { req: { id: c.card.id } });
  dump.learning_events = await call("learning_events");
  dump.current_practice = await call("current_practice");
  dump.learning_profile = await call("learning_profile");
  dump.coach[line[1].id] = await call("coach_view", { req: { study_id: A, node_id: line[1].id } });
  dump.answers = await call("coach_answers", { req: { study_id: A, node_id: line[3].id, mine, reference: line[4].mv } });
  dump.game_report = await call("game_report", { id: A });
  dump.backup_info = await call("backup_info");
  if (profile === "ab20742") dump.inbox_list = await call("inbox_list");
  // 同一份内容不同时刻读出的时间（复习的“今天”等）以这里为准
  dump.read_at = Date.now();
  fs.writeFileSync(path.join(out, "old-api.json"), JSON.stringify(dump, null, 2));
  bridgeProc.kill("SIGTERM");
  await new Promise((r) => bridgeProc.once("exit", r));
}
llm.kill();
// 日志里有本机路径，不放进样本
fs.rmSync(path.join(data, "logs"), { recursive: true, force: true });
console.log(`样本已写入 ${out}`);
