import { lazy, Suspense, type ComponentType, type LazyExoticComponent } from "react";
import { createRoot } from "react-dom/client";
import { ErrorBoundary } from "./ErrorBoundary";
import "./styles.css";

// 开发时在普通浏览器里预览界面
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  (await import("./dev/mock")).installMock();
}

const view = location.hash.slice(1) || "settings";
document.documentElement.dataset.view = view;

// 屏蔽浏览器默认的右键菜单、刷新等行为
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!(t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement)) e.preventDefault();
});
window.addEventListener("keydown", (e) => {
  if (e.key === "F5" || (e.ctrlKey && e.key.toLowerCase() === "r") || (e.ctrlKey && e.key.toLowerCase() === "p")) e.preventDefault();
});

// 每个窗口只加载自己的界面代码
const named = <T extends Record<string, ComponentType>>(load: () => Promise<T>, name: keyof T) =>
  lazy(() => load().then((m) => ({ default: m[name] })));

const views: Record<string, LazyExoticComponent<ComponentType>> = {
  capture: named(() => import("./views/CaptureView"), "CaptureView"),
  pin: named(() => import("./views/PinView"), "PinView"),
  ocr: named(() => import("./views/OcrView"), "OcrView"),
  translate: named(() => import("./views/OcrView"), "OcrView"),
  image: named(() => import("./views/ImageView"), "ImageView"),
  "scroll-frame": named(() => import("./views/ScrollViews"), "ScrollFrame"),
  "scroll-ctrl": named(() => import("./views/ScrollViews"), "ScrollCtrl"),
  settings: named(() => import("./views/SettingsView"), "SettingsView"),
};
const View = views[view] ?? views.settings;

createRoot(document.getElementById("root")!).render(
  <ErrorBoundary>
    <Suspense fallback={null}>
      <View />
    </Suspense>
  </ErrorBoundary>,
);
