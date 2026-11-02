import type Konva from "konva";
import { commitText } from "./drawing";
import { useEditor } from "./store";
import type { Rect } from "./types";

/** 合成「底图 + 批注」为 PNG。r 为舞台坐标（CSS 像素），输出为物理像素 */
export async function exportPng(bg: HTMLCanvasElement, layer: Konva.Layer | null, r: Rect, dpr: number): Promise<Uint8Array> {
  const st = useEditor.getState();
  // 去掉选中高亮，等 React 和 Konva 重绘
  if (st.selectedId || st.textEdit) {
    if (st.textEdit) commitText();
    st.select(null);
    await new Promise((res) => setTimeout(res, 30));
  }
  const px = Math.round(r.x * dpr);
  const py = Math.round(r.y * dpr);
  const pw = Math.max(1, Math.round(r.w * dpr));
  const ph = Math.max(1, Math.round(r.h * dpr));
  const out = document.createElement("canvas");
  out.width = pw;
  out.height = ph;
  const ctx = out.getContext("2d")!;
  ctx.drawImage(bg, px, py, pw, ph, 0, 0, pw, ph);
  if (layer && useEditor.getState().shapes.length) {
    layer.draw();
    const anno = layer.toCanvas({ x: r.x, y: r.y, width: r.w, height: r.h, pixelRatio: dpr });
    ctx.drawImage(anno, 0, 0, pw, ph);
  }
  const blob = await new Promise<Blob>((res, rej) => out.toBlob((b) => (b ? res(b) : rej(new Error("导出失败"))), "image/png"));
  return new Uint8Array(await blob.arrayBuffer());
}
