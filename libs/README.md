# libs 目录

运行和打包所需的外部资源。仓库只包含引擎；识别模型与 ONNX Runtime 运行库需要自行放置。

| 目录 | 内容 | 是否必需 |
|---|---|---|
| `pikafish/` | Pikafish 2026-09-06 引擎（官方发布的通用构建：Linux/Windows 为 x86-64，macOS 含 x86-64 与 arm64，运行时按 CPU 选择指令集）、`pikafish.nnue` 与许可文件 | 分析与讲解必需 |
| `models/` | 屏幕识别模型（如 `large.onnx`、`rotate.onnx`）与 `manifest.json` | 仅屏幕识别需要 |
| `windows-cpu/`、`windows-gpu/` | Windows 版 ONNX Runtime 动态库（`onnxruntime.dll` 等），打包时复制到安装目录 | 仅屏幕识别需要 |
| `linux/` | Linux 版 ONNX Runtime 动态库（`libonnxruntime*.so`） | 仅屏幕识别需要 |

缺少识别模型或运行库时，应用仍可启动，手动摆棋、FEN、棋谱导入与逐着讲解都可正常使用，
状态栏会说明屏幕识别不可用的原因。

引擎与网络文件取自 Pikafish 官方发布 [Pikafish 2026-09-06](https://github.com/official-pikafish/Pikafish/releases/tag/Pikafish-2026-09-06)
（`Pikafish.2026-09-06.7z`，SHA-256 `41952bbfe2520faceb5902c69e6ab4845cc999841d2b49a95cc1be7867a25e5b`），文件改名如下，内容未改动：

- `Pikafish-Linux-x86-64-universal` → `pikafish-linux`
- `Pikafish-Windows-x86-64-universal.exe` → `pikafish-windows.exe`
- `Pikafish-MacOS-universal` → `pikafish-macos`

Pikafish 以 GPL-3.0 许可发布（[Copying.txt](./pikafish/Copying.txt)，作者见 [AUTHORS](./pikafish/AUTHORS)），对应源码即上面这个发布的标签。
网络文件 `pikafish.nnue` 另有许可（[NNUE-License.md](./pikafish/NNUE-License.md)）：仅限合法用途，未经许可不得商用。
Linux 版引擎需要 glibc 2.38 及以上（如 Ubuntu 24.04）。
