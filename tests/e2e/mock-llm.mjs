// 模拟 DeepSeek 接口：根据请求中的证据包生成合格（或故意不合格）的讲解；证据包带有问题（教练追问）时生成回答。
// POST /__mode {"mode":"valid"|"bad", "delay_ms": 0} 切换输出与回复前的延迟；
// POST /__mode {"script": ["503", "bad", "valid", ...]} 之后的请求依次按脚本回复（503 为服务端错误），用完后按 mode；
// POST /__reset 恢复默认并清空记录；
// GET /__calls 返回收到的请求（含 Authorization 头、这一着的记法、追问的问题类型与收到的时间）。
import http from "node:http";

const port = Number(process.env.MOCK_LLM_PORT ?? 1440);
let mode = "valid";
let delay = 0;
let script = [];
const calls = [];

function pick(claims, groups) {
  return claims.find((c) => groups.includes(c.group))?.id;
}

function output(pack, how = mode) {
  const claims = pack.claims;
  if (how === "bad") {
    return { summary: { text: "这步是最佳着法，胜率很高。", claim_ids: ["c999"] }, sections: [], lesson: { text: "", claim_ids: [] }, followups: [] };
  }
  const sections = [
    { kind: "purpose", text: "开局阶段没有直接威胁，主要任务是尽快出动子力。", claim_ids: [pick(claims, ["need"])] },
    { kind: "reason", text: `${pack.move_notation}让炮来到中路，可以从中路施加压力。`, claim_ids: [pick(claims, ["basis"])] },
  ];
  const cmp = pick(claims, ["candidate", "comparison"]);
  if (cmp) sections.push({ kind: "comparison", text: "与其他候选的差别可以看下方的候选比较。", claim_ids: [cmp] });
  const reply = pick(claims, ["reply"]);
  if (reply) sections.push({ kind: "reply", text: "接下来要留意对方出马保护中卒。", claim_ids: [reply] });
  return {
    summary: { text: `${pack.move_notation}：架起中炮，开始出动子力。`, claim_ids: [pick(claims, ["basis"])] },
    sections,
    lesson: { text: "出子时检查新增的攻击和保护关系。", claim_ids: [pick(claims, ["lesson"])] },
    followups: [{ action: "show_reply", label: "演示对方的应对" }],
  };
}

/** 追问的回答：引用与问题相符的结论 */
function answer(pack, how = mode) {
  if (how === "bad") return { answer: { text: "这步更好，胜率很高。", claim_ids: ["c999"] }, points: [], check: "" };
  const group = { my_move: ["mine"], difference: ["difference"], reply: ["mine_reply"] }[pack.question.kind];
  const main = pick(pack.claims, group);
  return {
    answer: { text: `${pack.mine.notation}改变了这一侧的子力关系，接下来要留意对方怎样出子。`, claim_ids: [main] },
    points: [{ text: "对照选择改变了什么，可以对照下方程序给出的依据。", claim_ids: [pick(pack.claims, ["reference"]) ?? main] }],
    check: "现在哪些棋子在保护中路？",
  };
}

http
  .createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      if (req.url === "/__mode") {
        const m = JSON.parse(body);
        mode = m.mode ?? mode;
        delay = m.delay_ms ?? delay;
        script = m.script ?? script;
        res.end("{}");
        return;
      }
      if (req.url === "/__reset") {
        mode = "valid";
        delay = 0;
        script = [];
        calls.length = 0;
        res.end("{}");
        return;
      }
      if (req.url === "/__calls") {
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify(calls));
        return;
      }
      const request = JSON.parse(body || "{}");
      const pack = JSON.parse(request.messages[1].content).evidence;
      const asking = !!pack.question;
      calls.push({
        url: req.url,
        authorization: req.headers.authorization ?? null,
        notation: asking ? pack.mine.notation : pack.move_notation,
        question: asking ? pack.question.kind : null,
        at: Date.now(),
        body: request,
      });
      // 按收到请求时的设置回复
      const how = script.length ? script.shift() : mode;
      const content = how === "503" ? null : JSON.stringify(asking ? answer(pack, how) : output(pack, how));
      setTimeout(() => {
        res.setHeader("Content-Type", "application/json");
        if (content === null) {
          res.statusCode = 503;
          res.end(JSON.stringify({ error: { message: "service busy" } }));
          return;
        }
        res.end(JSON.stringify({ choices: [{ message: { role: "assistant", content }, finish_reason: "stop" }] }));
      }, delay);
    });
  })
  .listen(port, "127.0.0.1", () => console.log(`mock llm on ${port}`));
