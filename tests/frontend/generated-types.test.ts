// 由 Rust 生成的类型（src/api/generated）都要从入口导出，界面通过 types.ts 使用它们。
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

test("每个生成的类型都从入口导出", () => {
  const dir = new URL("../../src/api/generated/", import.meta.url);
  const files = readdirSync(dir).filter((f) => f.endsWith(".ts") && f !== "index.ts");
  const index = readFileSync(new URL("index.ts", dir), "utf8");
  const missing = files.map((f) => f.replace(/\.ts$/, "")).filter((name) => !index.includes(`from "./${name}"`));
  assert.ok(files.length > 0, "还没有生成的类型");
  assert.deepEqual(missing, [], `入口 src/api/generated/index.ts 缺少：${missing.join("、")}`);
});
