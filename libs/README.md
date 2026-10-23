# libs 目录

运行和打包所需的外部资源。仓库只包含引擎；识别模型与 ONNX Runtime 运行库需要自行放置。

| 目录 | 内容 | 是否必需 |
|---|---|---|
| `pikafish/` | Pikafish 引擎（Linux/Windows 为 x86-64-bmi2 构建，macOS 为 arm64）与 `pikafish.nnue` | 分析与讲解必需 |
| `models/` | 屏幕识别模型（如 `large.onnx`、`rotate.onnx`）与 `manifest.json` | 仅屏幕识别需要 |
| `windows-cpu/`、`windows-gpu/` | Windows 版 ONNX Runtime 动态库（`onnxruntime.dll` 等），打包时复制到安装目录 | 仅屏幕识别需要 |
| `linux/` | Linux 版 ONNX Runtime 动态库（`libonnxruntime*.so`） | 仅屏幕识别需要 |

缺少识别模型或运行库时，应用仍可启动，手动摆棋、FEN、棋谱导入与逐着讲解都可正常使用，
状态栏会说明屏幕识别不可用的原因。

Pikafish 以 GPL-3.0 许可发布，源码见 <https://github.com/official-pikafish/Pikafish>。
