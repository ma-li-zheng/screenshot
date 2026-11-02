import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import { errorText, outputImage, shotUrl, type ImageWin, type OcrLine, type TranslateResult } from "../lib/ipc";
import { FONT_FAMILY } from "../editor/types";

type Status = "loading" | "ok" | "error";

/** 提取文字 / 翻译 结果窗口 */
export function OcrView() {
  const [info, setInfo] = useState<ImageWin | null>(null);
  const [lines, setLines] = useState<OcrLine[]>([]);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState("");
  const [text, setText] = useState("");
  const [tr, setTr] = useState<TranslateResult | null>(null);
  const [trStatus, setTrStatus] = useState<Status | "idle">("idle");
  const [trError, setTrError] = useState("");
  const [showTranslated, setShowTranslated] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const [scale, setScale] = useState(1);
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const trCanvas = useRef<HTMLCanvasElement>(null);

  const say = (t: string) => {
    setToast(t);
    setTimeout(() => setToast(null), 1500);
  };

  const runTranslate = async (ls: OcrLine[]) => {
    if (!ls.length) return;
    setTrStatus("loading");
    setTrError("");
    try {
      const r = await invoke<TranslateResult>("translate_texts", { texts: ls.map((l) => l.text) });
      setTr(r);
      setTrStatus("ok");
      setShowTranslated(true);
    } catch (e) {
      setTrError(errorText(e));
      setTrStatus("error");
    }
  };

  useEffect(() => {
    invoke<ImageWin | null>("image_info").then(async (v) => {
      if (!v) return;
      setInfo(v);
      try {
        const r = await invoke<OcrLine[]>("ocr_image", { key: v.key });
        setLines(r);
        setText(r.map((l) => l.text).join("\n"));
        setStatus("ok");
        if (v.mode === "translate") runTranslate(r);
      } catch (e) {
        setError(errorText(e));
        setStatus("error");
      }
    });
  }, []);

  // 计算图片显示缩放，用于叠加可选中文字
  useEffect(() => {
    const el = imgRef.current;
    if (!el || !info) return;
    const ro = new ResizeObserver(() => setScale(el.clientWidth / info.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, [info, img]);

  // 生成译文覆盖图
  useEffect(() => {
    const c = trCanvas.current;
    if (!c || !img || !tr || !info) return;
    c.width = info.width;
    c.height = info.height;
    const ctx = c.getContext("2d", { willReadFrequently: true })!;
    ctx.drawImage(img, 0, 0);
    lines.forEach((l, i) => {
      const t = tr.texts[i];
      if (!t) return;
      const xs = l.box.map((p) => p[0]);
      const ys = l.box.map((p) => p[1]);
      const x = Math.max(0, Math.floor(Math.min(...xs)));
      const y = Math.max(0, Math.floor(Math.min(...ys)));
      const w = Math.min(info.width - x, Math.ceil(Math.max(...xs)) - x);
      const h = Math.min(info.height - y, Math.ceil(Math.max(...ys)) - y);
      if (w <= 2 || h <= 2) return;
      const [bg, fg] = boxColors(ctx, x, y, w, h);
      ctx.fillStyle = bg;
      ctx.fillRect(x, y, w, h);
      let fs = Math.max(8, h * 0.78);
      ctx.font = `${fs}px ${FONT_FAMILY}`;
      while (fs > 8 && ctx.measureText(t).width > w) {
        fs -= 0.5;
        ctx.font = `${fs}px ${FONT_FAMILY}`;
      }
      ctx.fillStyle = fg;
      ctx.textBaseline = "middle";
      ctx.fillText(t, x, y + h / 2, w);
    });
  }, [img, tr, lines, info]);

  const overlay = useMemo(() => {
    const measure = document.createElement("canvas").getContext("2d")!;
    return lines.map((l, i) => {
      const [p0, p1, , p3] = l.box;
      const left = p0[0] * scale;
      const top = p0[1] * scale;
      const w = Math.hypot(p1[0] - p0[0], p1[1] - p0[1]) * scale;
      const h = Math.hypot(p3[0] - p0[0], p3[1] - p0[1]) * scale;
      const fs = Math.max(6, h * 0.8);
      measure.font = `${fs}px ${FONT_FAMILY}`;
      const nat = measure.measureText(l.text).width || 1;
      return (
        <span
          key={i}
          className="ocr-word"
          style={{ left, top, height: h, fontSize: fs, lineHeight: `${h}px`, transform: `scaleX(${w / nat})` }}
        >
          {l.text}
        </span>
      );
    });
  }, [lines, scale]);

  const exportTranslated = async (action: "copy" | "save" | "pin") => {
    const c = trCanvas.current;
    if (!c) return;
    const blob = await new Promise<Blob | null>((r) => c.toBlob(r, "image/png"));
    if (!blob) return;
    const ok = await outputImage(new Uint8Array(await blob.arrayBuffer()), action, window.screenX * devicePixelRatio + 60, window.screenY * devicePixelRatio + 60);
    if (ok && action === "copy") say("译文图已复制");
  };

  const copy = async (t: string) => {
    await invoke("copy_text", { text: t });
    say("已复制");
  };

  if (!info) return null;
  const translatedText = tr ? tr.texts.join("\n") : "";
  return (
    <div className="ocr-root">
      <div className="ocr-left">
        <div className="ocr-imgbox">
          <div className="ocr-imgwrap" style={{ display: showTranslated && tr ? "none" : undefined }}>
            <img ref={imgRef} src={shotUrl(info.key)} onLoad={(e) => setImg(e.currentTarget)} draggable={false} style={{ maxWidth: "100%", width: info.width / devicePixelRatio }} />
            <div className="ocr-overlay">{overlay}</div>
          </div>
          <canvas
            ref={trCanvas}
            className="ocr-trcanvas"
            style={{ display: showTranslated && tr ? undefined : "none", maxWidth: "100%", width: info.width / devicePixelRatio }}
          />
        </div>
        {tr && (
          <div className="ocr-left-bar">
            <label>
              <input type="checkbox" checked={showTranslated} onChange={(e) => setShowTranslated(e.target.checked)} /> 显示译文图
            </label>
            <span className="grow" />
            <button onClick={() => exportTranslated("copy")}>复制译文图</button>
            <button onClick={() => exportTranslated("save")}>保存</button>
            <button onClick={() => exportTranslated("pin")}>钉到屏幕</button>
          </div>
        )}
      </div>
      <div className="ocr-right">
        {status === "loading" && <div className="ocr-status">正在识别文字…</div>}
        {status === "error" && <div className="ocr-error">识别失败：{error}</div>}
        {status === "ok" && (
          <>
            <div className="ocr-section-title">
              识别结果 <span className="muted">{lines.length} 行，可直接编辑</span>
            </div>
            <textarea className="ocr-text" value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} />
            <div className="ocr-actions">
              <button onClick={() => copy(text)} disabled={!text}>
                复制全部
              </button>
              <button className="primary" onClick={() => runTranslate(lines)} disabled={!lines.length || trStatus === "loading"}>
                {trStatus === "loading" ? "翻译中…" : tr ? "重新翻译" : "翻译"}
              </button>
            </div>
            {trStatus === "error" && (
              <div className="ocr-error">
                {trError}
                <div>
                  <button onClick={() => invoke("open_settings")}>打开设置</button>
                </div>
              </div>
            )}
            {tr && (
              <>
                <div className="ocr-section-title">
                  译文{" "}
                  <span className="muted">
                    → {tr.to === "zh" ? "中文" : "英文"} · {tr.provider}
                  </span>
                </div>
                <textarea className="ocr-text" value={translatedText} readOnly />
                <div className="ocr-actions">
                  <button onClick={() => copy(translatedText)}>复制译文</button>
                </div>
              </>
            )}
            {lines.length === 0 && <div className="ocr-status">没有识别到文字</div>}
          </>
        )}
      </div>
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

/** 取文字框边缘像素的中位色作为背景，按亮度选文字颜色 */
function boxColors(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number): [string, string] {
  const d = ctx.getImageData(x, y, w, h).data;
  const rs: number[] = [];
  const gs: number[] = [];
  const bs: number[] = [];
  const push = (px: number, py: number) => {
    const i = (py * w + px) * 4;
    rs.push(d[i]);
    gs.push(d[i + 1]);
    bs.push(d[i + 2]);
  };
  for (let px = 0; px < w; px++) {
    push(px, 0);
    push(px, h - 1);
  }
  for (let py = 0; py < h; py++) {
    push(0, py);
    push(w - 1, py);
  }
  const med = (a: number[]) => a.sort((p, q) => p - q)[a.length >> 1];
  const [r, g, b] = [med(rs), med(gs), med(bs)];
  const lum = 0.299 * r + 0.587 * g + 0.114 * b;
  return [`rgb(${r},${g},${b})`, lum > 140 ? "#1f1f1f" : "#ffffff"];
}
