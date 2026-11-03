// 与后端 serde 结构一一对应的类型：全部由 Rust 结构生成（ts-rs，见仓库根目录的 .cargo/config.toml），
// 运行 `cargo test` 时写到 src/api/generated，CI 检查生成的文件与 Rust 代码一致。这里不再手写协议类型，
// 后端结构改变时类型检查会直接指出界面中需要跟着改的地方。字段名保持后端的 snake_case。
export type * from "./generated";
