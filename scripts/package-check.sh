#!/usr/bin/env bash
# 检查 Linux 安装包（deb，在打包流程中运行，需要 sudo）：
# 1. 全新安装，冒烟测试（带空格与中文的新数据目录、应用自己的目录各一次）；
# 2. 卸载：程序删除，学习数据保留；
# 3. 从 v0.3.0 的 deb 原地升级：放入 v0.3.0 格式的学习数据（历史样本），检查迁移与内容；
# 每次冒烟测试都检查版本一致、随安装包发布的引擎与许可清单都在、引擎能就绪。
#
# 用法：scripts/package-check.sh <这次的 deb> [v0.3.0 的 deb]
set -euo pipefail
deb=$(realpath "$1")
previous=${2:+$(realpath "$2")}
root=$(cd "$(dirname "$0")/.." && pwd)
expected=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$root/server/tauri.conf.json")
identifier=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["identifier"])' "$root/server/tauri.conf.json")
work="${RUNNER_TEMP:-/tmp}/安装包 检查"
mkdir -p "$work"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/$identifier"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/$identifier"

# 冒烟测试并检查报告：smoke 名称 [数据目录]，报告写在 $work/名称.json
smoke() {
  local name=$1 report="$work/$1.json" status=0
  if [ $# -ge 2 ]; then xqlink --smoke "$report" --data "$2" > /dev/null || status=$?; else xqlink --smoke "$report" > /dev/null || status=$?; fi
  cat "$report"
  echo
  python3 - "$report" "$expected" "$status" <<'PY'
import json, sys
r = json.load(open(sys.argv[1])); expected = sys.argv[2]; status = int(sys.argv[3])
problems = list(r.get("problems", []))
if status != 0 or not r.get("ok"): problems.append(f"冒烟测试没有通过（退出码 {status}）")
if r.get("version") != expected or r.get("app_version") != expected: problems.append(f"版本不一致：安装包 {expected}，报告 {r.get('version')}，应用内 {r.get('app_version')}")
if r.get("schema", {}).get("schema_version", 0) < 2: problems.append("数据版本不是当前版本")
if problems: sys.exit("；".join(problems))
PY
}

field() { python3 -c 'import json,sys; v=json.load(open(sys.argv[1])); [v:=v[k] if not k.isdigit() else v[int(k)] for k in sys.argv[2].split(".")]; print(v if not isinstance(v,list) else len(v))' "$work/$1.json" "$2"; }

echo "::group::全新安装"
sudo apt-get install -y "$deb"
command -v xqlink
smoke smoke-clean "$work/全新 数据"
[ "$(field smoke-clean schema.migrations)" = 0 ] && [ "$(field smoke-clean studies)" = 0 ] || { echo "全新的数据目录不应有迁移或学习数据"; exit 1; }
smoke smoke-appdata
[ -f "$data_dir/xqlink.redb" ] || { echo "应用自己的数据目录中没有数据库：$data_dir"; exit 1; }
echo "::endgroup::"

echo "::group::卸载"
sudo apt-get remove -y xqlink
# 之前运行过 xqlink，bash 记住了它的位置：先清掉，再看程序是否真的删除了
hash -r
if dpkg -s xqlink > /dev/null 2>&1 || command -v xqlink; then echo "卸载后程序仍在"; exit 1; fi
[ -f "$data_dir/xqlink.redb" ] || { echo "卸载删除了学习数据"; exit 1; }
rm -rf "$data_dir" "$config_dir"
echo "::endgroup::"

if [ -n "$previous" ]; then
  echo "::group::从 v0.3.0 升级"
  sudo apt-get install -y "$previous"
  mkdir -p "$data_dir" "$config_dir"
  cp -r "$root/server/crates/xq-app/tests/history/v0.3.0/home/data/." "$data_dir/"
  cp -r "$root/server/crates/xq-app/tests/history/v0.3.0/home/config/." "$config_dir/"
  sudo apt-get install -y "$deb"
  smoke smoke-upgrade
  [ "$(field smoke-upgrade schema.migrations)" = 1 ] && [ "$(field smoke-upgrade schema.migrations.0.from)" = 1 ] || { echo "升级后应从版本 1 迁移一次"; exit 1; }
  [ "$(field smoke-upgrade studies)" = 2 ] && [ "$(field smoke-upgrade reviews)" = 4 ] || { echo "升级后学习数据不完整"; exit 1; }
  ls "$data_dir"/backups/xqlink-schema1-*.redb
  smoke smoke-upgrade-again
  [ "$(field smoke-upgrade-again schema.migrations)" = 1 ] || { echo "第二次启动又迁移了一次"; exit 1; }
  sudo apt-get remove -y xqlink
  echo "::endgroup::"
else
  echo "::notice::没有 v0.3.0 的 deb，跳过升级检查"
fi
echo "安装包检查通过（版本 $expected）"
