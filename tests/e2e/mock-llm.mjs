// 模拟 DeepSeek 接口：根据请求中的证据包生成合格（或故意不合格）的讲解。
// POST /__mode {"mode":"valid"|"bad", "delay_ms": 0} 切换输出与回复前的延迟；POST /__reset 恢复默认并清空记录；
// GET /__calls 返回收到的请求（含 Authorization 头、这一着的记法与收到的时间）。
import http from "node:http";

const port = Number(process.env.MOCK_LLM_PORT ?? 1440);
let mode = "valid";
let delay = 0;
const calls = [];

function pick(claims, groups) {
  return claims.find((c) => groups.includes(c.group))?.id;
}

function output(pack) {
  const claims = pack.claims;
  if (mode === "bad") {
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

http
  .createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      if (req.url === "/__mode") {
        const m = JSON.parse(body);
        mode = m.mode ?? mode;
        delay = m.delay_ms ?? delay;
        res.end("{}");
        return;
      }
      if (req.url === "/__reset") {
        mode = "valid";
        delay = 0;
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
      calls.push({ url: req.url, authorization: req.headers.authorization ?? null, notation: pack.move_notation, at: Date.now(), body: request });
      // 按收到请求时的设置回复
      const content = JSON.stringify(output(pack));
      setTimeout(() => {
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify({ choices: [{ message: { role: "assistant", content }, finish_reason: "stop" }] }));
      }, delay);
    });
  })
  .listen(port, "127.0.0.1", () => console.log(`mock llm on ${port}`));
