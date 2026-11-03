// 第三方许可清单：列出安装包中带上的第三方程序与数据（Pikafish 引擎与网络文件），以及桌面程序依赖的 Rust crate
// 与前端 npm 包（只算随程序发布的：Rust 为 xqlink 的普通依赖，不含开发与构建用的依赖；npm 为生产依赖），
// 写到仓库根目录的 THIRD_PARTY_LICENSES.md，并随安装包分发。
//
// 用法：node scripts/third-party-licenses.mjs [--check]
//   --check  不写文件：许可不在允许的列表中、没有写明许可，或提交的清单不是最新的时失败（CI 使用）
// 环境变量 CARGO、PNPM 可以指定命令（默认 cargo、pnpm）。
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { allowed } from "./license-expr.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(repo, "THIRD_PARTY_LICENSES.md");
const check = process.argv.includes("--check");

/** 按名称、版本排列（不按语言环境，清单在各台机器上相同） */
const byNameVersion = (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : a.version < b.version ? -1 : a.version > b.version ? 1 : 0);

function rustCrates() {
  const cargo = process.env.CARGO ?? "cargo";
  const meta = JSON.parse(
    execFileSync(cargo, ["metadata", "--format-version", "1", "--locked", "--manifest-path", path.join(repo, "server", "Cargo.toml")], {
      encoding: "utf8",
      maxBuffer: 1 << 28,
    }),
  );
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const root = meta.packages.find((p) => p.name === "xqlink" && meta.workspace_members.includes(p.id));
  const workspace = new Set(meta.workspace_members);
  // 从 xqlink 沿普通依赖走（各平台的依赖都算上，清单在不同系统上相同）
  const seen = new Set();
  const stack = [root.id];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id)?.deps ?? []) {
      if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
    }
  }
  return [...seen]
    .filter((id) => !workspace.has(id))
    .map((id) => byId.get(id))
    .map((p) => ({ name: p.name, version: p.version, license: p.license ?? null }))
    .sort((a, b) => byNameVersion(a, b));
}

function npmPackages() {
  const pnpm = process.env.PNPM ?? "pnpm";
  const [cmd, ...pre] = pnpm.split(" ");
  const list = JSON.parse(execFileSync(cmd, [...pre, "licenses", "list", "--prod", "--json"], { cwd: repo, encoding: "utf8", maxBuffer: 1 << 26 }));
  const out = [];
  for (const [license, pkgs] of Object.entries(list)) {
    for (const p of pkgs) for (const version of p.versions) out.push({ name: p.name, version, license: license === "Unknown" ? null : license });
  }
  return out.sort((a, b) => byNameVersion(a, b));
}

const rows = (list) => list.map((p) => `| ${p.name} | ${p.version} | ${p.license ?? "（没有写明）"} |`).join("\n");

const rust = rustCrates();
const npm = npmPackages();
const text = `# 第三方许可清单

由 \`node scripts/third-party-licenses.mjs\` 生成，不要手工修改；CI 检查它是最新的，并且每一项的许可都在允许的列表中。

## 随安装包分发的程序与数据

| 名称 | 许可 | 说明 |
| --- | --- | --- |
| Pikafish 2026-09-06（引擎） | GPL-3.0 | 许可全文：libs/pikafish/Copying.txt；作者：libs/pikafish/AUTHORS；对应源码：https://github.com/official-pikafish/Pikafish/releases/tag/Pikafish-2026-09-06 |
| pikafish.nnue（网络文件） | 见 libs/pikafish/NNUE-License.md | 仅限合法用途，未经许可不得商用 |

## 桌面程序依赖的 Rust crate（${rust.length} 个）

| 名称 | 版本 | 许可 |
| --- | --- | --- |
${rows(rust)}

## 界面依赖的 npm 包（${npm.length} 个）

| 名称 | 版本 | 许可 |
| --- | --- | --- |
${rows(npm)}
`;

const bad = [...rust, ...npm].filter((p) => !p.license || !allowed(p.license));
if (check) {
  const problems = [];
  if (bad.length) problems.push(`许可没有写明或不在允许的列表中：\n${bad.map((p) => `  ${p.name} ${p.version}：${p.license ?? "没有写明"}`).join("\n")}`);
  const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, "utf8") : "";
  if (current !== text) problems.push("THIRD_PARTY_LICENSES.md 不是最新的：运行 node scripts/third-party-licenses.mjs 后提交");
  if (problems.length) {
    console.error(problems.join("\n"));
    process.exit(1);
  }
  console.log(`第三方许可清单是最新的：Rust ${rust.length} 个、npm ${npm.length} 个，许可都在允许的列表中`);
} else {
  fs.writeFileSync(OUT, text);
  console.log(`已写入 ${path.relative(repo, OUT)}：Rust ${rust.length} 个、npm ${npm.length} 个${bad.length ? `；${bad.length} 个许可需要确认` : ""}`);
  for (const p of bad) console.log(`  需要确认：${p.name} ${p.version}：${p.license ?? "没有写明"}`);
}
