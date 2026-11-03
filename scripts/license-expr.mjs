// 许可表达式（SPDX）是否在允许的列表中：third-party-licenses.mjs 使用，单独放在这里便于测试。

/** 允许的许可（SPDX）：宽松的开源许可，可以随本程序分发 */
const ALLOWED = new Set([
  "MIT",
  "MIT-0",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "Unicode-3.0",
  "Unicode-DFS-2016",
  "MPL-2.0",
  "CC0-1.0",
  "0BSD",
  "BSL-1.0",
  "Unlicense",
  "CDLA-Permissive-2.0",
]);

/** 许可表达式是否满足：OR 中有一项允许即可，AND 要求每一项都允许，“X WITH 例外”按 X 判断。旧写法的“/”当作 OR */
export function allowed(expr) {
  const tokens = expr.replace(/\s*\/\s*/g, " OR ").replace(/[()]/g, (p) => ` ${p} `).split(/\s+/).filter(Boolean);
  let i = 0;
  const term = () => {
    if (tokens[i] === "(") {
      i += 1;
      const v = or();
      i += 1; // ")"
      return v;
    }
    const id = tokens[i++];
    if (tokens[i] === "WITH") i += 2;
    return ALLOWED.has(id);
  };
  const and = () => {
    let v = term();
    while (tokens[i] === "AND") {
      i += 1;
      v = term() && v;
    }
    return v;
  };
  const or = () => {
    let v = and();
    while (tokens[i] === "OR") {
      i += 1;
      v = and() || v;
    }
    return v;
  };
  return tokens.length > 0 && or() && i === tokens.length;
}

