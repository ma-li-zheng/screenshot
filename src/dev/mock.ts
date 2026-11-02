// 仅开发时使用：在普通浏览器里预览界面，用假的 Tauri 后端返回示例数据
const settings = {
  hotkeyCapture: "Alt+Shift+A",
  hotkeyPin: "Alt+Shift+P",
  saveDir: "C:\\Users\\me\\Pictures\\Screenshot",
  imageFormat: "png",
  fileNamePrefix: "Screenshot",
  autostart: false,
  translateEngine: "offline",
  offlineFallback: false,
  modelMirror: "modelscope",
  translateProvider: "openai",
  translateTarget: "auto",
  azureRegion: "",
  openaiBaseUrl: "https://api.deepseek.com/v1",
  openaiModel: "deepseek-chat",
  quota: { tencent: 5000000, baidu: 1000000, azure: 2000000, openai: 0 },
  quotaRatio: 0.95,
  usage: { tencent: { month: "2026-11", chars: 1234 } },
};

const responses: Record<string, unknown> = {
  get_settings: { settings, month: "2026-11" },
  get_secret: {},
  model_status: [
    { direction: "zh-en", installed: true, downloading: false, size: 119494000, path: "D:\\software\\Screenshot\\models\\opus-mt-zh-en" },
    { direction: "en-zh", installed: false, downloading: false, size: 0, path: "D:\\software\\Screenshot\\models\\opus-mt-en-zh" },
  ],
};

export function installMock() {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string) => responses[cmd] ?? null,
    transformCallback: () => 0,
    metadata: { currentWindow: { label: "preview" }, currentWebview: { label: "preview", windowLabel: "preview" } },
  };
}
