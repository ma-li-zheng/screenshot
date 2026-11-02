import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type Konva from "konva";
import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { Layer, Rect as KRect, Stage } from "react-konva";
import { X } from "lucide-react";
import { AnnotationLayer } from "../editor/AnnotationLayer";
import { beginDraw, editorKey, endDraw, moveDraw } from "../editor/drawing";
import { exportPng } from "../editor/exportImage";
import { useEditor } from "../editor/store";
import { TextEditor } from "../editor/TextEditor";
import { Toolbar, type ToolbarAction } from "../editor/Toolbar";
import type { Rect } from "../editor/types";
import { clamp, normRect } from "../editor/types";
import { errorText, outputImage, shotUrl, type OutputAction } from "../lib/ipc";

interface R4 {
  x: number;
  y: number;
  w: number;
  h: number;
}
interface MonitorInfo {
  index: number;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  key: string;
  windows: { rect: R4; children: R4[] }[];
}
interface CaptureInfo {
  session: number;
  monitor: MonitorInfo;
  focus: boolean;
  cursor: [number, number] | null;
  /** shot：截图；record：选择录屏区域 */
  mode: string;
}
interface Session {
  id: number;
  monitors: MonitorInfo[];
  focus: number;
  cursor: [number, number];
  mode: string;
}

type Phase = "idle" | "hover" | "pressing" | "selecting" | "selected";
type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
type Op =
  | { kind: "move"; sx: number; sy: number; orig: Rect }
  | { kind: "resize"; handle: Handle; orig: Rect }
  | { kind: "draw" }
  | null;

const HANDLE_CURSOR: Record<Handle, string> = {
  nw: "nwse-resize",
  se: "nwse-resize",
  ne: "nesw-resize",
  sw: "nesw-resize",
  n: "ns-resize",
  s: "ns-resize",
  e: "ew-resize",
  w: "ew-resize",
};

const win = getCurrentWebviewWindow();
const MemoAnno = memo(AnnotationLayer);

function handlesOf(r: Rect): [Handle, number, number][] {
  const { x, y, w, h } = r;
  return [
    ["nw", x, y],
    ["n", x + w / 2, y],
    ["ne", x + w, y],
    ["e", x + w, y + h / 2],
    ["se", x + w, y + h],
    ["s", x + w / 2, y + h],
    ["sw", x, y + h],
    ["w", x, y + h / 2],
  ];
}

function hitHandle(r: Rect | null, x: number, y: number): Handle | null {
  if (!r) return null;
  for (const [hd, hx, hy] of handlesOf(r)) if (Math.abs(x - hx) <= 6 && Math.abs(y - hy) <= 6) return hd;
  return null;
}

/** 把选区对齐到物理像素，避免导出尺寸出现 ±1 */
function snap(r: Rect): Rect {
  const d = window.devicePixelRatio || 1;
  const x = Math.round(r.x * d);
  const y = Math.round(r.y * d);
  const w = Math.round((r.x + r.w) * d) - x;
  const h = Math.round((r.y + r.h) * d) - y;
  return { x: x / d, y: y / d, w: w / d, h: h / d };
}

/** 选区外的四块遮罩 */
function maskRects(hole: Rect | null, W: number, H: number): Rect[] {
  if (!hole) return [{ x: 0, y: 0, w: W, h: H }];
  const { x, y, w, h } = hole;
  return [
    { x: 0, y: 0, w: W, h: y },
    { x: 0, y: y + h, w: W, h: H - y - h },
    { x: 0, y, w: x, h },
    { x: x + w, y, w: W - x - w, h },
  ].filter((r) => r.w > 0 && r.h > 0);
}

const inside = (r: Rect | null, x: number, y: number) => !!r && x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;

export function CaptureView() {
  const bgRef = useRef<HTMLCanvasElement>(null);
  const layerRef = useRef<Konva.Layer>(null);
  const toolbarRef = useRef<HTMLDivElement>(null);
  const pixels = useRef<Uint8ClampedArray | null>(null);
  const session = useRef(0);
  const press = useRef<{ x: number; y: number } | null>(null);
  const op = useRef<Op>(null);

  const [info, setInfo] = useState<CaptureInfo | null>(null);
  const [size, setSize] = useState({ w: window.innerWidth, h: window.innerHeight });
  const [phase, setPhase] = useState<Phase>("idle");
  const [sel, setSel] = useState<Rect | null>(null);
  const [hover, setHover] = useState<Rect | null>(null);
  const [mouse, setMouse] = useState<{ x: number; y: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [cursor, setCursor] = useState("crosshair");
  const [toast, setToast] = useState<string | null>(null);
  const [tbSize, setTbSize] = useState({ w: 640, h: 40 });
  const tool = useEditor((s) => s.tool);
  const busy = useRef(false);

  const dpr = window.devicePixelRatio || 1;
  const W = size.w;
  const H = size.h;

  // 让键盘回调能读到最新状态
  const latest = useRef({ phase, sel, info, mouse, dpr });
  latest.current = { phase, sel, info, mouse, dpr };

  const findRect = useCallback(
    (m: MonitorInfo, x: number, y: number): Rect => {
      const d = window.devicePixelRatio || 1;
      const X = x * d;
      const Y = y * d;
      const has = (r: R4) => X >= r.x && X < r.x + r.w && Y >= r.y && Y < r.y + r.h;
      let best: R4 = { x: 0, y: 0, w: m.width, h: m.height };
      for (const wi of m.windows) {
        if (!has(wi.rect)) continue;
        best = wi.rect;
        for (const c of wi.children) if (has(c) && c.w * c.h < best.w * best.h) best = c;
        break;
      }
      const x1 = Math.max(0, best.x);
      const y1 = Math.max(0, best.y);
      const x2 = Math.min(m.width, best.x + best.w);
      const y2 = Math.min(m.height, best.y + best.h);
      return { x: x1 / d, y: y1 / d, w: (x2 - x1) / d, h: (y2 - y1) / d };
    },
    [],
  );

  const resetLocal = useCallback(() => {
    useEditor.getState().reset();
    setPhase("idle");
    setSel(null);
    setHover(null);
    setMouse(null);
    setToast(null);
    op.current = null;
    press.current = null;
    busy.current = false;
    setDragging(false);
  }, []);

  const begin = useCallback(
    async (v: CaptureInfo) => {
      if (session.current === v.session) return;
      session.current = v.session;
      resetLocal();
      const m = v.monitor;
      const buf = await (await fetch(shotUrl(m.key))).arrayBuffer();
      const data = new Uint8ClampedArray(buf);
      const c = bgRef.current!;
      c.width = m.width;
      c.height = m.height;
      c.getContext("2d")!.putImageData(new ImageData(data, m.width, m.height), 0, 0);
      pixels.current = data;
      setSize({ w: window.innerWidth, h: window.innerHeight });
      setInfo(v);
      setPhase("hover");
      if (v.cursor) {
        const d = window.devicePixelRatio || 1;
        const p = { x: v.cursor[0] / d, y: v.cursor[1] / d };
        setMouse(p);
        setHover(findRect(m, p.x, p.y));
      }
      await win.show();
      if (v.focus) await win.setFocus();
    },
    [findRect, resetLocal],
  );

  useEffect(() => {
    const un1 = listen<Session>("capture:begin", (e) => {
      const idx = Number(win.label.replace("overlay-", ""));
      const s = e.payload;
      const m = s.monitors[idx];
      if (!m) return;
      begin({ session: s.id, monitor: m, focus: s.focus === idx, cursor: s.focus === idx ? s.cursor : null, mode: s.mode });
    });
    const un2 = listen("capture:end", () => {
      resetLocal();
      setInfo(null);
      const c = bgRef.current;
      if (c) c.getContext("2d")?.clearRect(0, 0, c.width, c.height);
    });
    // 窗口刚创建时可能错过事件，主动查询一次
    invoke<CaptureInfo | null>("capture_info").then((v) => v && begin(v));
    const onResize = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", onResize);
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
      window.removeEventListener("resize", onResize);
    };
  }, [begin, resetLocal]);

  const finish = useCallback(() => {
    invoke("capture_end");
  }, []);

  const act = useCallback(
    async (a: ToolbarAction) => {
      const { sel, info, dpr } = latest.current;
      const st = useEditor.getState();
      if (a === "undo") return st.undo();
      if (a === "redo") return st.redo();
      if (a === "cancel") return finish();
      if (!sel || !info || busy.current) return;
      const m = info.monitor;
      const sx = m.x + Math.round(sel.x * dpr);
      const sy = m.y + Math.round(sel.y * dpr);
      if (a === "long") {
        await invoke("scroll_start", { x: sx, y: sy, w: Math.round(sel.w * dpr), h: Math.round(sel.h * dpr) });
        return;
      }
      if (a === "record") {
        await invoke("record_start", { x: sx, y: sy, w: Math.round(sel.w * dpr), h: Math.round(sel.h * dpr) });
        return;
      }
      const action: OutputAction = a === "done" ? "copy" : (a as OutputAction);
      busy.current = true;
      try {
        const png = await exportPng(bgRef.current!, layerRef.current, sel, dpr);
        const ok = await outputImage(png, action, sx, sy);
        busy.current = false;
        if (ok) finish();
      } catch (e) {
        busy.current = false;
        setToast(errorText(e));
      }
    },
    [finish],
  );

  // 键盘
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { phase, sel, mouse, info, dpr } = latest.current;
      if (!info) return;
      if (useEditor.getState().textEdit) return;
      const ctrl = e.ctrlKey || e.metaKey;
      if (e.key === "Escape") {
        e.preventDefault();
        return finish();
      }
      if (phase === "selected") {
        if (info.mode === "record") {
          if (e.key === "Enter") act("record");
          return;
        }
        if (e.key === "Enter" || (ctrl && e.key.toLowerCase() === "c")) return void act("done");
        if (ctrl && e.key.toLowerCase() === "s") {
          e.preventDefault();
          return void act("save");
        }
        if (editorKey(e)) return e.preventDefault();
        if (sel && e.key.startsWith("Arrow")) {
          e.preventDefault();
          const step = (e.shiftKey ? 10 : 1) / dpr;
          const dx = e.key === "ArrowLeft" ? -step : e.key === "ArrowRight" ? step : 0;
          const dy = e.key === "ArrowUp" ? -step : e.key === "ArrowDown" ? step : 0;
          setSel({
            ...sel,
            x: clamp(sel.x + dx, 0, window.innerWidth - sel.w),
            y: clamp(sel.y + dy, 0, window.innerHeight - sel.h),
          });
        }
        return;
      }
      // 选区前：按 C 复制光标处颜色
      if (phase === "hover" && mouse && e.key.toLowerCase() === "c" && pixels.current) {
        const px = Math.floor(mouse.x * dpr);
        const py = Math.floor(mouse.y * dpr);
        const i = (py * info.monitor.width + px) * 4;
        const p = pixels.current;
        const hex = "#" + [p[i], p[i + 1], p[i + 2]].map((v) => v.toString(16).padStart(2, "0")).join("").toUpperCase();
        invoke("copy_text", { text: hex });
        setToast(`已复制颜色 ${hex}`);
        setTimeout(() => setToast(null), 1200);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [act, finish]);

  // 工具栏实际尺寸
  useLayoutEffect(() => {
    const el = toolbarRef.current;
    if (!el) return;
    const bar = el.querySelector(".toolbar") as HTMLElement | null;
    if (bar && (bar.offsetWidth !== tbSize.w || bar.offsetHeight !== tbSize.h)) {
      setTbSize({ w: bar.offsetWidth, h: bar.offsetHeight });
    }
  });

  const pos = (e: Konva.KonvaEventObject<MouseEvent>) => e.target.getStage()!.getPointerPosition()!;

  const onDown = (e: Konva.KonvaEventObject<MouseEvent>) => {
    if (e.evt.button !== 0 || !info) return;
    const p = pos(e);
    if (phase === "hover") {
      press.current = p;
      setPhase("pressing");
      return;
    }
    if (phase === "selected" && sel) {
      const hd = hitHandle(sel, p.x, p.y);
      if (hd) {
        op.current = { kind: "resize", handle: hd, orig: sel };
        setDragging(true);
        return;
      }
      if (beginDraw(p.x, p.y, e.target, sel)) {
        op.current = { kind: "draw" };
        return;
      }
      if (!useEditor.getState().tool && inside(sel, p.x, p.y)) {
        op.current = { kind: "move", sx: p.x, sy: p.y, orig: sel };
        setDragging(true);
      }
    }
  };

  const onMove = (e: Konva.KonvaEventObject<MouseEvent>) => {
    if (!info) return;
    const p = pos(e);
    setMouse(p);
    if (phase === "hover") {
      setHover(findRect(info.monitor, p.x, p.y));
      return;
    }
    if (phase === "pressing" && press.current) {
      if (Math.hypot(p.x - press.current.x, p.y - press.current.y) > 4) {
        setPhase("selecting");
        setSel(snap(normRect(press.current.x, press.current.y, p.x, p.y)));
      }
      return;
    }
    if (phase === "selecting" && press.current) {
      setSel(snap(normRect(press.current.x, press.current.y, clamp(p.x, 0, W), clamp(p.y, 0, H))));
      return;
    }
    if (phase !== "selected" || !sel) return;
    const o = op.current;
    if (o?.kind === "draw") {
      moveDraw(p.x, p.y, sel, e.evt.shiftKey);
    } else if (o?.kind === "move") {
      setSel(
        snap({
          ...o.orig,
          x: clamp(o.orig.x + p.x - o.sx, 0, W - o.orig.w),
          y: clamp(o.orig.y + p.y - o.sy, 0, H - o.orig.h),
        }),
      );
    } else if (o?.kind === "resize") {
      let { x: x1, y: y1 } = o.orig;
      let x2 = o.orig.x + o.orig.w;
      let y2 = o.orig.y + o.orig.h;
      const px = clamp(p.x, 0, W);
      const py = clamp(p.y, 0, H);
      if (o.handle.includes("w")) x1 = px;
      if (o.handle.includes("e")) x2 = px;
      if (o.handle.includes("n")) y1 = py;
      if (o.handle.includes("s")) y2 = py;
      setSel(snap(normRect(x1, y1, x2, y2)));
    } else {
      // 更新光标形状
      const hd = hitHandle(sel, p.x, p.y);
      const t = useEditor.getState().tool;
      setCursor(hd ? HANDLE_CURSOR[hd] : t === "text" ? "text" : t ? "crosshair" : inside(sel, p.x, p.y) ? "move" : "default");
    }
  };

  const onUp = () => {
    if (phase === "pressing") {
      setSel(hover ?? { x: 0, y: 0, w: W, h: H });
      setPhase("selected");
      setCursor("move");
    } else if (phase === "selecting") {
      if (!sel || sel.w < 3 || sel.h < 3) {
        setSel(null);
        setPhase("hover");
      } else {
        setPhase("selected");
      }
    } else if (op.current?.kind === "draw") {
      endDraw();
    }
    press.current = null;
    op.current = null;
    setDragging(false);
  };

  const onDbl = (e: Konva.KonvaEventObject<MouseEvent>) => {
    if (phase !== "selected" || useEditor.getState().tool) return;
    const p = pos(e);
    if (inside(sel, p.x, p.y)) act(info?.mode === "record" ? "record" : "done");
  };

  const onContext = (e: Konva.KonvaEventObject<PointerEvent>) => {
    e.evt.preventDefault();
    if (useEditor.getState().textEdit) return;
    if (phase === "selected" || phase === "selecting") {
      // 右键：清除选区重新选
      useEditor.getState().reset();
      setSel(null);
      setPhase("hover");
      setCursor("crosshair");
      if (info && mouse) setHover(findRect(info.monitor, mouse.x, mouse.y));
    } else {
      finish();
    }
  };

  const hole = phase === "hover" ? hover : sel;
  const showMagnifier = !!info && !!mouse && (phase === "hover" || phase === "pressing" || phase === "selecting" || op.current?.kind === "resize");
  const showToolbar = phase === "selected" && !!sel && !dragging;

  // 工具栏位置：选区右下方外侧 → 上方 → 选区内
  let tbStyle: CSSProperties = {};
  let propsBelow = true;
  if (sel && showToolbar) {
    const gap = 6;
    const total = tbSize.h + (tool ? 40 : 0);
    const left = clamp(sel.x + sel.w - tbSize.w, 4, W - tbSize.w - 4);
    if (sel.y + sel.h + gap + total <= H) {
      tbStyle = { left, top: sel.y + sel.h + gap };
    } else if (sel.y - gap - total >= 0) {
      tbStyle = { left, top: sel.y - gap - total };
      propsBelow = false;
    } else {
      tbStyle = { left, top: Math.max(4, sel.y + sel.h - gap - total) };
    }
  }

  return (
    <div className="capture-root" style={{ cursor: phase === "selected" ? cursor : "crosshair" }}>
      <canvas ref={bgRef} className="capture-bg" style={{ width: W, height: H }} />
      {info && (
        <Stage
          width={W}
          height={H}
          onMouseDown={onDown}
          onMouseMove={onMove}
          onMouseUp={onUp}
          onDblClick={onDbl}
          onContextMenu={onContext}
          onMouseLeave={() => {
            if (phase === "hover") {
              setMouse(null);
              setHover(null);
            }
          }}
        >
          <MemoAnno ref={layerRef} bg={bgRef.current} dpr={dpr} clip={sel} />
          <Layer listening={false}>
            {maskRects(hole, W, H).map((r, i) => (
              <KRect key={i} x={r.x} y={r.y} width={r.w} height={r.h} fill="rgba(0,0,0,0.45)" />
            ))}
            {hole && (
              <KRect
                x={hole.x}
                y={hole.y}
                width={hole.w}
                height={hole.h}
                stroke="#1677FF"
                strokeWidth={phase === "hover" ? 3 : 1.5}
              />
            )}
            {phase === "selected" &&
              sel &&
              handlesOf(sel).map(([hd, hx, hy]) => (
                <KRect key={hd} x={hx - 3.5} y={hy - 3.5} width={7} height={7} fill="#fff" stroke="#1677FF" strokeWidth={1.5} />
              ))}
          </Layer>
        </Stage>
      )}
      {sel && (phase === "selecting" || phase === "selected") && (
        <div className="size-label" style={{ left: sel.x, top: sel.y >= 26 ? sel.y - 26 : sel.y + 4 }}>
          {Math.round(sel.w * dpr)} × {Math.round(sel.h * dpr)}
        </div>
      )}
      {showMagnifier && <Magnifier mouse={mouse!} bg={bgRef.current} pixels={pixels.current} monitor={info!.monitor} W={W} H={H} />}
      {showToolbar && (
        <div ref={toolbarRef}>
          {info?.mode === "record" ? (
            <div className="toolbar-wrap" style={tbStyle}>
              <div className="toolbar record-bar" onMouseDown={(e) => e.stopPropagation()}>
                <button title="退出 (Esc)" className="danger" onClick={() => act("cancel")}>
                  <X size={18} />
                </button>
                <button title="开始录制 (Enter)" className="primary rec" onClick={() => act("record")}>
                  <span className="rec-dot" />
                  <span>开始录制</span>
                </button>
              </div>
            </div>
          ) : (
            <Toolbar mode="capture" style={tbStyle} onAction={act} propsBelow={propsBelow} />
          )}
        </div>
      )}
      {info?.mode === "record" && phase === "hover" && (
        <div className="record-hint">拖拽选择录制区域，单击选择窗口或整个屏幕 · Esc 退出</div>
      )}
      <TextEditor />
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function Magnifier({
  mouse,
  bg,
  pixels,
  monitor,
  W,
  H,
}: {
  mouse: { x: number; y: number };
  bg: HTMLCanvasElement | null;
  pixels: Uint8ClampedArray | null;
  monitor: MonitorInfo;
  W: number;
  H: number;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const dpr = window.devicePixelRatio || 1;
  const N = 15;
  const SIZE = 120;
  const px = clamp(Math.floor(mouse.x * dpr), 0, monitor.width - 1);
  const py = clamp(Math.floor(mouse.y * dpr), 0, monitor.height - 1);
  const i = (py * monitor.width + px) * 4;
  const [r, g, b] = pixels ? [pixels[i], pixels[i + 1], pixels[i + 2]] : [0, 0, 0];

  useLayoutEffect(() => {
    const c = ref.current;
    if (!c || !bg) return;
    const s = Math.round(SIZE * dpr);
    c.width = s;
    c.height = s;
    const ctx = c.getContext("2d")!;
    ctx.imageSmoothingEnabled = false;
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, s, s);
    ctx.drawImage(bg, px - (N >> 1), py - (N >> 1), N, N, 0, 0, s, s);
    const cell = s / N;
    ctx.strokeStyle = "rgba(255,255,255,0.12)";
    ctx.lineWidth = 1;
    for (let k = 1; k < N; k++) {
      ctx.beginPath();
      ctx.moveTo(k * cell, 0);
      ctx.lineTo(k * cell, s);
      ctx.moveTo(0, k * cell);
      ctx.lineTo(s, k * cell);
      ctx.stroke();
    }
    ctx.fillStyle = "rgba(22,119,255,0.35)";
    const mid = (N >> 1) * cell;
    ctx.fillRect(mid, 0, cell, mid);
    ctx.fillRect(mid, mid + cell, cell, s - mid - cell);
    ctx.fillRect(0, mid, mid, cell);
    ctx.fillRect(mid + cell, mid, s - mid - cell, cell);
    ctx.strokeStyle = "#fff";
    ctx.strokeRect(mid, mid, cell, cell);
  }, [bg, px, py, dpr]);

  const boxW = SIZE + 2;
  const boxH = SIZE + 62;
  let left = mouse.x + 20;
  let top = mouse.y + 20;
  if (left + boxW > W) left = mouse.x - 20 - boxW;
  if (top + boxH > H) top = mouse.y - 20 - boxH;
  const hex = "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("").toUpperCase();
  return (
    <div className="magnifier" style={{ left, top }}>
      <canvas ref={ref} style={{ width: SIZE, height: SIZE }} />
      <div className="mag-info">
        <div>
          坐标 {px}, {py}
        </div>
        <div className="mag-color">
          <span className="swatch" style={{ background: hex }} />
          RGB({r},{g},{b})
        </div>
        <div className="mag-tip">按 C 复制 {hex}</div>
      </div>
    </div>
  );
}
