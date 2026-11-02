// 仅开发时使用：在普通浏览器里预览界面，用假的 Tauri 后端返回示例数据
const settings = {
  hotkeyCapture: "Alt+Shift+A",
  hotkeyPin: "Alt+Shift+P",
  hotkeyRecord: "Alt+Shift+R",
  recordFps: 30,
  recordQuality: "standard",
  recordCursor: true,
  recordSystemAudio: true,
  recordMic: false,
  saveDir: "C:\\Users\\me\\Pictures\\Screenshot",
  imageFormat: "png",
  fileNamePrefix: "Screenshot",
  autostart: false,
  translateProvider: "openai",
  translateTarget: "auto",
  azureRegion: "",
  openaiBaseUrl: "https://api.deepseek.com/v1",
  openaiModel: "deepseek-chat",
  quota: { tencent: 5000000, baidu: 1000000, azure: 2000000, openai: 0 },
  quotaRatio: 0.95,
  usage: { tencent: { month: "2026-11", chars: 1234 } },
};

// 示例 OCR 结果（对应 src-tauri/tests_ocr_sample.png）
const box = (x: number, y: number, w: number, h: number) => [
  [x, y],
  [x + w, y],
  [x + w, y + h],
  [x, y + h],
];
const ocrLines = [
  { text: "截图工具支持文字识别和翻译功能。", score: 0.99, box: box(38, 30, 360, 34) },
  { text: "Screenshot tool supports OCR and translation.", score: 0.98, box: box(38, 100, 470, 34) },
  { text: "订单编号：20261102-8848，金额￥1,299.00", score: 0.98, box: box(38, 170, 440, 34) },
  { text: "小号文字也需要能够识别出来，例如12px的说明.", score: 0.97, box: box(38, 238, 330, 24) },
];

const responses: Record<string, unknown> = {
  image_info: { key: "tests_ocr_sample.png", width: 900, height: 360, mode: location.hash.slice(1) || "ocr" },
  ocr_image: ocrLines,
  translate_texts: {
    texts: ["Screenshot tools support OCR and translation.", "截图工具支持 OCR 和翻译。", "Order number: 20261102-8848, amount 1,299.00", "Small text should also be recognized, e.g. 12px notes."],
    to: "en",
    provider: "腾讯云翻译",
  },
  get_settings: { settings, month: "2026-11" },
  get_secret: {},
  record_status: { recording: true, paused: false, seconds: 83.4, hasSys: true, hasMic: true, muteSys: false, muteMic: true, error: null },
  file_info: { id: "1", path: "C:\Users\me\Pictures\Screenshot\Recording_20261102_150102.mp4", name: "Recording_20261102_150102.mp4", size: 3456789 },
};

export function installMock() {
  // 用不同的源（127.0.0.1 与 localhost）提供图片，模拟 shot:// 协议的跨源情况
  (window as unknown as Record<string, unknown>).__SHOT_BASE__ = "http://127.0.0.1:1420/src-tauri/";
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      // 记录最后一次调用，方便在预览里检查
      (window as unknown as Record<string, unknown>).__lastInvoke = { cmd, args };
      if (cmd === "gif_save") {
        (window as unknown as Record<string, unknown>).__gifBytes = args;
        return "2";
      }
      return responses[cmd] ?? null;
    },
    transformCallback: () => 0,
    metadata: { currentWindow: { label: "preview" }, currentWebview: { label: "preview", windowLabel: "preview" } },
  };
}
