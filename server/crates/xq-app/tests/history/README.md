# 历史数据样本

旧版本实际写出的数据目录、导出文件与配置文件。`tests/history.rs` 用当前代码打开它们，检查迁移只做一次、内容不变、
导出再导入仍相同，以及来自更新版本的数据被拒绝。

**样本生成后不再修改。** 每个文件的校验值记在 `SHA256SUMS` 中，测试会检查；需要新的样本时另建一个目录，
再更新 `SHA256SUMS`（`find . -type f ! -name SHA256SUMS ! -name README.md | sed 's|^\./||' | LC_ALL=C sort | xargs sha256sum > SHA256SUMS`）。

## 样本

| 目录 | 来源 | 内容 |
|---|---|---|
| `v0.3.0/` | 正式发布的 v0.3.0（`81ae068`） | 见下文“完整样本的内容” |
| `v0.3.0-interrupted/` | v0.3.0 | 前半部分与完整样本相同；之后在 AI 讲解与追问生成中、两着讲解进行中、一次局面比较等待引擎时强制结束进程 |
| `ab20742-inbox/` | 0.4 开发版（`ab20742`，复盘收件箱，加出题出处与修改次数之前） | 完整样本的内容，加上“我的名字”设置与收件箱中分析完的两盘棋 |
| `v0.1.2-config/` | 0.1.2（上游版本） | 两份配置文件：默认设置，以及搜索时间被重复换算单位的设置 |

完整样本的内容：

- 导入的对局（含对局信息），之后改名、设定我执红方；一着的笔记与收藏；一条分支；删除过的另一条分支（节点编号不再使用）；
- 先想后讲的练习记录（看了一级提示，第一次没有选对）；
- 两张复习卡片：一张复现棋谱没有选对、标为再练并写了笔记与问题；一张局面决策，比较已经完成；
- 残局会话中出的一道找威胁，以及加入复习后**删除了原会话**的卡片；
- 一组没有做完的练习：答了第一题，停在第二题；
- 一着的 AI 讲解与一次教练追问的回答（模拟模型服务）；
- 执黑、棋盘翻转的第三个会话；
- `archive.json`、`study-export.json`：同一时刻用旧版本导出的学习档案与单个会话；`home/data/backups/` 中是当天的自动备份；
- `old-api.json`：旧版本用自己的接口读出的全部内容，是与当前版本比较的依据（只保留了版本与设置，去掉了本机路径）；
- `home/config/credentials.json` 中的密钥是虚构的。

数据目录的布局与应用相同：`home/data/xqlink.redb`（数据库，版本 1）、`home/data/backups/`、`home/config/`。

## 生成方法

`scripts/history-fixtures.mjs` 启动旧版本的开发桥（使用旧版本仓库中的 Pikafish，单线程、固定深度）与旧版本的模拟模型服务，
通过开发桥的命令完成上面的操作。例如 v0.3.0：

```bash
git worktree add /tmp/v030 v0.3.0
CARGO_TARGET_DIR=/tmp/target-v030 cargo build --manifest-path /tmp/v030/server/Cargo.toml -p xq-devserver
node scripts/history-fixtures.mjs --devserver /tmp/target-v030/debug/xq-devserver \
  --mock-llm /tmp/v030/tests/e2e/mock-llm.mjs --profile v0.3.0 --out server/crates/xq-app/tests/history/v0.3.0
# 中断的样本加 --interrupted；0.4 开发版用 ab20742 的工作区与 --profile ab20742
```

`v0.1.2-config/` 中的两份文件按 0.1.2 的 `Config::save`（`serde_json::to_string_pretty`）的格式写成：0.1.2 是只有界面的程序，
没有在生成环境中运行。第二份的搜索时间 `8000000` 是 0.1.2 的界面把 8 秒换算成毫秒后、后台又乘了 1000 的结果。
