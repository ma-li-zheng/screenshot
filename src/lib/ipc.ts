import { invoke } from "@tauri-apps/api/core";

/** shot:// 协议在 Windows 上的访问地址 */
export function shotUrl(key: string): string {
  return `http://shot.localhost/${key}`;
}

export type OutputAction = "copy" | "save" | "pin" | "ocr" | "translate";

/** 把 PNG 交给后端：复制 / 保存 / 钉到屏幕 / 文字识别 / 翻译。返回 false 表示用户取消了保存 */
export function outputImage(png: Uint8Array, action: OutputAction, x = 0, y = 0): Promise<boolean> {
  return invoke<boolean>("output_image", png, {
    headers: { "x-action": action, "x-x": String(Math.round(x)), "x-y": String(Math.round(y)) },
  });
}

export interface ImageWin {
  key: string;
  width: number;
  height: number;
  mode: string;
}

export interface OcrLine {
  text: string;
  score: number;
  box: [number, number][];
}

export interface TranslateResult {
  texts: string[];
  to: string;
  provider: string;
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
