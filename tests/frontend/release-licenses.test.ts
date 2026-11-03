// 发布的许可检查：安装包带上的第三方文件都附有许可。
// - 带 Pikafish 引擎或网络文件的平台，同时带上 GPL-3.0 全文、作者名单与网络文件的许可；
// - 带上第三方许可清单 THIRD_PARTY_LICENSES.md（依赖的 Rust crate 与 npm 包，CI 检查它是最新的、许可都允许）；
// - 配置中写明的文件都在仓库中；
// - 按目录带上的文件（识别模型、ONNX Runtime）目前只有说明文件；放进程序库或模型时，同一目录中要有它的许可文件。
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

const server = new URL("../../server/", import.meta.url);
const configs = readdirSync(server).filter((f) => /^tauri\..+\.conf\.json$/.test(f));
const resourcesOf = (file: string): string[] => Object.keys(JSON.parse(readFileSync(new URL(file, server), "utf8")).bundle?.resources ?? {});
const PIKAFISH_NOTICES = ["../libs/pikafish/Copying.txt", "../libs/pikafish/NNUE-License.md", "../libs/pikafish/AUTHORS"];
/** 说明与清单，不是第三方的程序或数据 */
const isNote = (name: string) => /^README.*\.md$/i.test(name) || name === "manifest.json";
const isLicense = (name: string) => /licen[cs]e|copying|notice/i.test(name);

test("每个平台的安装包都检查过", () => {
  assert.ok(configs.length >= 4, configs.join("、"));
});

for (const file of configs) {
  test(`${file}：带上的第三方文件都附有许可`, () => {
    const resources = resourcesOf(file);
    if (resources.some((r) => r.startsWith("../libs/pikafish/"))) {
      const missing = PIKAFISH_NOTICES.filter((n) => !resources.includes(n));
      assert.deepEqual(missing, [], "带 Pikafish 时要带上它的许可文件");
    }
    assert.ok(resources.includes("../THIRD_PARTY_LICENSES.md"), "带上第三方许可清单（见 scripts/third-party-licenses.mjs）");
    for (const r of resources) {
      if (r.endsWith("/*")) {
        const dir = new URL(r.slice(0, -1), server);
        const names = existsSync(dir) ? readdirSync(dir) : [];
        const payload = names.filter((n) => !isNote(n) && !isLicense(n));
        assert.ok(!payload.length || names.some(isLicense), `${r} 中有 ${payload.join("、")}，但没有许可文件`);
      } else {
        assert.ok(existsSync(new URL(r, server)), `${r} 不在仓库中`);
      }
    }
  });
}
