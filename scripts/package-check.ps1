# 检查 Windows 安装包（在打包流程中运行，需要管理员权限的 msiexec）：
# 1. 全新安装到带空格与中文的目录，冒烟测试（新的数据目录与应用自己的目录各一次）；
# 2. 卸载：程序删除，学习数据保留；
# 3. 从 v0.3.0 的安装包原地升级：先装 v0.3.0，放入 v0.3.0 格式的学习数据（历史样本），再装这次的安装包，
#    检查只有一份安装、数据迁移到当前版本且内容都在、迁移前的数据库有备份；
# 4. 每次冒烟测试都检查安装包、应用内与报告的版本一致，随安装包发布的引擎与许可清单都在，引擎能就绪。
#
# 用法：pwsh scripts/package-check.ps1 -Msi <这次的安装包> [-Previous <v0.3.0 的安装包>]
# 没有 -Previous 时跳过升级检查（例如仓库中没有 v0.3.0 的发布）。
param(
    [Parameter(Mandatory = $true)][string]$Msi,
    [string]$Previous = ""
)
$ErrorActionPreference = "Stop"
$conf = Get-Content (Join-Path $PSScriptRoot "../server/tauri.conf.json") -Raw | ConvertFrom-Json
$expected = $conf.version
$identifier = $conf.identifier
$work = Join-Path $env:RUNNER_TEMP "安装包 检查"
New-Item -ItemType Directory -Force $work | Out-Null
$appData = Join-Path $env:APPDATA $identifier

function Step($text) { Write-Host "::group::$text" }
function EndStep { Write-Host "::endgroup::" }

function Install-Msi([string]$file, [string]$target, [string]$name) {
    $log = Join-Path $work "$name.log"
    $args = @("/i", "`"$file`"", "/qn", "/norestart", "/l*v", "`"$log`"")
    if ($target) { $args += "INSTALLDIR=`"$target`"" }
    $p = Start-Process msiexec.exe -ArgumentList $args -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "安装 $name 失败（msiexec 退出码 $($p.ExitCode)），日志：$log" }
}

function Uninstall-Msi([string]$file, [string]$name) {
    $log = Join-Path $work "$name.log"
    $p = Start-Process msiexec.exe -ArgumentList @("/x", "`"$file`"", "/qn", "/norestart", "/l*v", "`"$log`"") -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "卸载 $name 失败（msiexec 退出码 $($p.ExitCode)），日志：$log" }
}

# 已安装的 xqlink（注册表中的卸载项）
function Installed {
    $keys = @("HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*", "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*")
    @(Get-ItemProperty $keys -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq "xqlink" })
}

function Smoke([string]$exe, [string]$name, [string]$data) {
    $report = Join-Path $work "$name.json"
    $args = @("--smoke", "`"$report`"")
    if ($data) { $args += @("--data", "`"$data`"") }
    $p = Start-Process $exe -ArgumentList $args -Wait -PassThru
    if (-not (Test-Path $report)) { throw "$name：冒烟测试没有写出报告（退出码 $($p.ExitCode)）" }
    $r = Get-Content $report -Raw -Encoding UTF8 | ConvertFrom-Json
    Write-Host (Get-Content $report -Raw -Encoding UTF8)
    if ($p.ExitCode -ne 0 -or -not $r.ok) { throw "$name：冒烟测试没有通过：$($r.problems -join '；')" }
    if ($r.version -ne $expected -or $r.app_version -ne $expected) { throw "$name：版本不一致（安装包 $expected，报告 $($r.version)，应用内 $($r.app_version)）" }
    if ($r.schema.schema_version -lt 2) { throw "$name：数据版本 $($r.schema.schema_version) 不是当前版本" }
    return $r
}

# 1. 全新安装到带空格与中文的目录
Step "全新安装"
$target = "C:\测试 目录\象棋 学习助手"
Install-Msi $Msi $target "install-clean"
$exe = Join-Path $target "xqlink.exe"
if (-not (Test-Path $exe)) { throw "安装目录中没有 xqlink.exe：$target" }
if ((Installed).Count -ne 1) { throw "应有一份安装，实际 $((Installed).Count) 份" }
$fresh = Smoke $exe "smoke-clean" (Join-Path $work "全新 数据")
if ($fresh.schema.migrations.Count -ne 0 -or $fresh.studies -ne 0) { throw "全新的数据目录不应有迁移或学习数据" }
$own = Smoke $exe "smoke-appdata" ""
if (-not (Test-Path (Join-Path $appData "xqlink.redb"))) { throw "应用自己的数据目录中没有数据库：$appData" }
EndStep

# 2. 卸载：程序删除，学习数据保留
Step "卸载"
Uninstall-Msi $Msi "uninstall-clean"
if (Test-Path $exe) { throw "卸载后程序仍在：$exe" }
if ((Installed).Count -ne 0) { throw "卸载后仍有安装记录" }
if (-not (Test-Path (Join-Path $appData "xqlink.redb"))) { throw "卸载删除了学习数据：$appData" }
Remove-Item -Recurse -Force $appData
EndStep

# 3. 从 v0.3.0 原地升级
if ($Previous) {
    Step "从 v0.3.0 升级"
    Install-Msi $Previous "" "install-previous"
    if ((Installed).Count -ne 1) { throw "v0.3.0 应有一份安装" }
    # v0.3.0 格式的学习数据：Windows 上数据目录与配置目录相同（%APPDATA%\标识符）
    $fixture = Join-Path $PSScriptRoot "../server/crates/xq-app/tests/history/v0.3.0/home"
    New-Item -ItemType Directory -Force $appData | Out-Null
    Copy-Item -Recurse -Force (Join-Path $fixture "data/*") $appData
    Copy-Item -Force (Join-Path $fixture "config/*") $appData
    Install-Msi $Msi "" "install-upgrade"
    $installed = Installed
    if ($installed.Count -ne 1) { throw "升级后应只有一份安装，实际 $($installed.Count) 份" }
    $exe = Join-Path $installed[0].InstallLocation "xqlink.exe"
    if (-not (Test-Path $exe)) {
        $exe = Get-ChildItem -Recurse -Filter xqlink.exe "C:\Program Files\xqlink", "C:\Program Files (x86)\xqlink" -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName
    }
    $up = Smoke $exe "smoke-upgrade" ""
    $m = @($up.schema.migrations)
    if ($m.Count -ne 1 -or $m[0].from -ne 1 -or $m[0].to -ne 2) { throw "升级后应从版本 1 迁移到 2 一次：$($up.schema | ConvertTo-Json -Depth 5)" }
    if ($up.studies -ne 2 -or $up.reviews -ne 4) { throw "升级后学习数据不完整：会话 $($up.studies)，卡片 $($up.reviews)" }
    if (-not (Get-ChildItem (Join-Path $appData "backups") -Filter "xqlink-schema1-*.redb" -ErrorAction SilentlyContinue)) { throw "没有迁移前的数据库备份" }
    # 再次启动不再迁移
    $again = Smoke $exe "smoke-upgrade-again" ""
    if (@($again.schema.migrations).Count -ne 1) { throw "第二次启动又迁移了一次" }
    Uninstall-Msi $Msi "uninstall-upgrade"
    EndStep
} else {
    Write-Host "::notice::没有 v0.3.0 的安装包，跳过升级检查"
}
Write-Host "安装包检查通过（版本 $expected）"
