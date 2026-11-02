# Screenshot 截图工具

Windows 桌面截图软件：快捷键截图、批注、钉在屏幕、离线文字识别（PaddleOCR）、翻译、长截图。

## 使用

| 操作 | 方式 |
|---|---|
| 截图 | `Alt+Shift+A`，或左键单击托盘图标 |
| 把剪贴板图片钉到屏幕 | `Alt+Shift+P` |
| 设置 | 右键托盘图标 → 设置 |

**截图时**

- 鼠标悬停会自动识别窗口，单击直接选中；按住拖拽则是自由选区
- 放大镜显示坐标和颜色，按 `C` 复制颜色值
- 选区可以拖动，8 个控制点可以调整大小，方向键微调（按住 Shift 每次 10 像素）
- 右键：有选区时重新选，没有选区时退出；`Esc` 退出
- `Enter`、双击选区或 `Ctrl+C`：完成并复制到剪贴板；`Ctrl+S` 保存
- 批注：矩形、椭圆、箭头、直线、画笔、荧光笔、文字、马赛克、模糊、序号
  - 画好的批注单击可以选中，然后拖动；`Delete` 删除
  - `Ctrl+Z` 撤销，`Ctrl+Y` 重做
  - 按住 Shift 画正方形、正圆，或把直线/箭头限制在 45° 方向

**贴图窗口**

- 拖动移动；滚轮缩放；`Ctrl+滚轮` 调透明度
- 双击或 `Esc` 关闭
- 右键菜单：复制、另存为、透明度、恢复原始大小、关闭全部贴图

**长截图**

- 选区后点工具栏的「长截图」，然后在选区内滚动鼠标滚轮，或点「自动滚动」
- 点「完成」后打开长图窗口，可以继续批注、复制、保存、钉图、OCR

**提取文字与翻译**

- 选区后点工具栏的「提取文字」，先显示识别结果；需要时再点窗口里的「翻译」
- 翻译使用在线 API：腾讯云、百度、Azure（都有每月免费额度），或 OpenAI 兼容接口（DeepSeek、通义千问、本机 Ollama 等）
- 在设置里选择服务并填写密钥；免费额度的服务会在本地统计每月用量，默认用到 95% 就暂停翻译

## 开发

开发环境：Node 20+、Rust stable（MSVC）、VS 2022 C++ 生成工具。

```bash
npm install
npm run tauri dev      # 开发运行
npm run tauri build    # 打包，产物在 src-tauri/target/release/bundle/nsis/
```

测试：

```bash
cd src-tauri
cargo test --lib -- --include-ignored   # 包括 OCR 识别测试和长截图拼接测试
```

## 目录

```
src/                 前端（React + TS + Konva）
  views/             各窗口：截图遮罩、贴图、OCR/翻译、长图、长截图控制条、设置
  editor/            批注编辑器：图形、绘制逻辑、工具栏、导出
src-tauri/src/       Rust 后端
  capture.rs         多屏截取、遮罩窗口
  win32.rs           枚举窗口、模拟滚轮、排除截屏
  stitch.rs          长截图拼接算法
  scroll.rs          长截图采集
  ocr.rs             PaddleOCR 检测 + 识别
  translate.rs       腾讯云 / 百度 / Azure / OpenAI 兼容接口翻译
src-tauri/resources/
  ocr/               PP-OCRv5 mobile 模型（det.onnx / rec.onnx / dict.txt）
  runtime/           随安装包分发的 VC++ 运行库、DirectML.dll
scripts/             测试辅助脚本（截屏、模拟键鼠）
```

模型来源：PaddlePaddle 官方 Hugging Face 仓库 `PaddlePaddle/PP-OCRv5_mobile_det_onnx`、`PaddlePaddle/PP-OCRv5_mobile_rec_onnx`（Apache-2.0）。
