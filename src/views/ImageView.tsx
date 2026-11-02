import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type Konva from "konva";
import { memo, useEffect, useRef, useState } from "react";
import { Stage } from "react-konva";
import { AnnotationLayer } from "../editor/AnnotationLayer";
import { beginDraw, editorKey, endDraw, moveDraw } from "../editor/drawing";
import { exportPng } from "../editor/exportImage";
import { useEditor } from "../editor/store";
import { TextEditor } from "../editor/TextEditor";
import { Toolbar, type ToolbarAction } from "../editor/Toolbar";
import type { Rect } from "../editor/types";
import { errorText, outputImage, shotUrl, type ImageWin, type OutputAction } from "../lib/ipc";

const win = getCurrentWebviewWindow();
const MemoAnno = memo(AnnotationLayer);

/** 长截图结果：可批注，支持复制/保存/钉图/OCR/翻译 */
export function ImageView() {
  const bgRef = useRef<HTMLCanvasElement>(null);
  const layerRef = useRef<Konva.Layer>(null);
  const [info, setInfo] = useState<ImageWin | null>(null);
  const [ready, setReady] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const drawing = useRef(false);
  const dpr = window.devicePixelRatio || 1;

  useEffect(() => {
    invoke<ImageWin | null>("image_info").then(async (v) => {
      if (!v) return;
      setInfo(v);
      const blob = await (await fetch(shotUrl(v.key))).blob();
      const bmp = await createImageBitmap(blob);
      const c = bgRef.current!;
      c.width = bmp.width;
      c.height = bmp.height;
      c.getContext("2d")!.drawImage(bmp, 0, 0);
      setReady(true);
    });
  }, []);

  const region: Rect | null = info ? { x: 0, y: 0, w: info.width / dpr, h: info.height / dpr } : null;

  const say = (t: string) => {
    setToast(t);
    setTimeout(() => setToast(null), 1500);
  };

  const act = async (a: ToolbarAction) => {
    const st = useEditor.getState();
    if (a === "undo") return st.undo();
    if (a === "redo") return st.redo();
    if (a === "cancel") return invoke("image_close");
    if (!region || !bgRef.current) return;
    const action: OutputAction = a === "done" ? "copy" : (a as OutputAction);
    try {
      const png = await exportPng(bgRef.current, layerRef.current, region, dpr);
      const pos = await win.outerPosition();
      const ok = await outputImage(png, action, pos.x + 40, pos.y + 40);
      if (!ok) return;
      if (action === "copy") say("已复制到剪贴板");
      if (action === "save") say("已保存");
    } catch (e) {
      say(errorText(e));
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;
      if (useEditor.getState().textEdit) return;
      if (ctrl && e.key.toLowerCase() === "c") act("done");
      else if (ctrl && e.key.toLowerCase() === "s") {
        e.preventDefault();
        act("save");
      } else if (editorKey(e)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const pos = (e: Konva.KonvaEventObject<MouseEvent>) => e.target.getStage()!.getPointerPosition()!;

  return (
    <div className="image-root">
      <div className="image-toolbar">
        <Toolbar mode="image" onAction={act} />
      </div>
      <div className="image-scroll">
        <div className="image-canvas" style={region ? { width: region.w, height: region.h } : undefined}>
          <canvas ref={bgRef} style={region ? { width: region.w, height: region.h } : undefined} />
          {ready && region && (
            <Stage
              className="image-stage"
              width={region.w}
              height={region.h}
              onMouseDown={(e) => {
                if (e.evt.button !== 0) return;
                const p = pos(e);
                drawing.current = beginDraw(p.x, p.y, e.target, region);
              }}
              onMouseMove={(e) => {
                if (!drawing.current) return;
                const p = pos(e);
                moveDraw(p.x, p.y, region, e.evt.shiftKey);
              }}
              onMouseUp={() => {
                if (drawing.current) endDraw();
                drawing.current = false;
              }}
            >
              <MemoAnno ref={layerRef} bg={bgRef.current} dpr={dpr} clip={region} />
            </Stage>
          )}
          <TextEditor />
        </div>
      </div>
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}
