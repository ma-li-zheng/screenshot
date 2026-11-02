import { invoke } from "@tauri-apps/api/core";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { Menu, Submenu } from "@tauri-apps/api/menu";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef, useState } from "react";
import { shotUrl, type ImageWin } from "../lib/ipc";

const win = getCurrentWebviewWindow();
/** 与后端 PIN_MARGIN 一致 */
const MARGIN = 8;

export function PinView() {
  const [info, setInfo] = useState<ImageWin | null>(null);
  const [zoom, setZoom] = useState(1);
  const [opacity, setOpacity] = useState(1);
  const [focused, setFocused] = useState(true);
  const [hint, setHint] = useState<string | null>(null);
  const hintTimer = useRef<number>(0);
  const lastDown = useRef({ t: 0, x: 0, y: 0 });
  const state = useRef({ zoom, opacity });
  state.current = { zoom, opacity };

  useEffect(() => {
    invoke<ImageWin | null>("image_info").then(setInfo);
    const un = win.onFocusChanged(({ payload }) => setFocused(payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const flash = (t: string) => {
    setHint(t);
    clearTimeout(hintTimer.current);
    hintTimer.current = window.setTimeout(() => setHint(null), 900);
  };

  const close = () => invoke("image_close");

  /** 以 (clientX, clientY) 为锚点缩放 */
  const applyZoom = async (next: number, cx?: number, cy?: number) => {
    if (!info) return;
    const dpr = window.devicePixelRatio || 1;
    const k = next / state.current.zoom;
    const m = Math.round(MARGIN * dpr);
    const w = Math.max(8, Math.round(info.width * next)) + 2 * m;
    const h = Math.max(8, Math.round(info.height * next)) + 2 * m;
    const pos = await win.outerPosition();
    const ax = cx ?? window.innerWidth / 2;
    const ay = cy ?? window.innerHeight / 2;
    const ux = (ax - MARGIN) * dpr;
    const uy = (ay - MARGIN) * dpr;
    const nx = Math.round(pos.x + ax * dpr - ux * k - m);
    const ny = Math.round(pos.y + ay * dpr - uy * k - m);
    setZoom(next);
    await win.setSize(new PhysicalSize(w, h));
    await win.setPosition(new PhysicalPosition(nx, ny));
    flash(`${Math.round(next * 100)}%`);
  };

  const applyOpacity = (v: number) => {
    const o = Math.min(1, Math.max(0.1, Math.round(v * 10) / 10));
    setOpacity(o);
    flash(`不透明度 ${Math.round(o * 100)}%`);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;
      if (e.key === "Escape") close();
      else if (ctrl && e.key.toLowerCase() === "c") invoke("pin_copy").then(() => flash("已复制"));
      else if (ctrl && e.key.toLowerCase() === "s") {
        e.preventDefault();
        invoke("pin_save");
      } else if (e.key === "0" && ctrl) applyZoom(1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const showMenu = async () => {
    const pct = (v: number) => ({
      id: `op${v}`,
      text: `${v}%`,
      action: () => applyOpacity(v / 100),
    });
    const menu = await Menu.new({
      items: [
        { id: "copy", text: "复制\tCtrl+C", action: () => invoke("pin_copy").then(() => flash("已复制")) },
        { id: "save", text: "另存为…\tCtrl+S", action: () => invoke("pin_save") },
        { item: "Separator" },
        await Submenu.new({ text: "不透明度", items: [pct(100), pct(80), pct(60), pct(40), pct(20)] }),
        { id: "zoom1", text: "恢复原始大小\tCtrl+0", action: () => applyZoom(1) },
        { item: "Separator" },
        { id: "close", text: "关闭\tEsc", action: () => close() },
        { id: "closeAll", text: "关闭全部贴图", action: () => invoke("pin_close_all") },
      ],
    });
    await menu.popup();
  };

  if (!info) return null;
  const dpr = window.devicePixelRatio || 1;
  return (
    <div
      className={`pin-root ${focused ? "focused" : ""}`}
      style={{ padding: MARGIN }}
      onMouseDown={(e) => {
        if (e.button !== 0) return;
        const now = Date.now();
        const l = lastDown.current;
        if (now - l.t < 400 && Math.abs(e.screenX - l.x) < 4 && Math.abs(e.screenY - l.y) < 4) {
          close();
          return;
        }
        lastDown.current = { t: now, x: e.screenX, y: e.screenY };
        win.startDragging();
      }}
      onWheel={(e) => {
        if (e.ctrlKey) applyOpacity(state.current.opacity + (e.deltaY < 0 ? 0.1 : -0.1));
        else {
          const z = state.current.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1);
          applyZoom(Math.min(8, Math.max(0.1, z)), e.clientX, e.clientY);
        }
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        showMenu();
      }}
    >
      <img
        src={shotUrl(info.key)}
        draggable={false}
        style={{ width: (info.width * zoom) / dpr, height: (info.height * zoom) / dpr, opacity }}
        onLoad={() => {
          win.show();
          win.setFocus();
        }}
      />
      {hint && <div className="pin-hint">{hint}</div>}
    </div>
  );
}
