// 第三方许可清单的许可判断：OR 中有一项允许即可，AND 要求都允许，WITH 例外按主许可判断。
import assert from "node:assert/strict";
import { test } from "node:test";
import { allowed } from "../../scripts/license-expr.mjs";

test("许可表达式", () => {
  for (const ok of ["MIT", "MIT OR Apache-2.0", "MIT/Apache-2.0", "Apache-2.0 / MIT", "(MIT OR Apache-2.0) AND Unicode-3.0", "Apache-2.0 WITH LLVM-exception", "MIT OR Apache-2.0 OR LGPL-2.1-or-later"]) {
    assert.ok(allowed(ok), ok);
  }
  for (const bad of ["GPL-3.0", "LGPL-2.1-or-later", "MIT AND GPL-3.0", "SSPL-1.0", "", "(MIT"]) {
    assert.ok(!allowed(bad), bad);
  }
});
