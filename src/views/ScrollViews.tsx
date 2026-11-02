import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Pause, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { shotUrl } from "../lib/ipc";

/** 长截图选区边框（透明、鼠标穿透、不会被截进图里） */
export function ScrollFrame() {
  return <div className="scroll-frame" />;
}

interface Progress {
  height: number;
  width: number;
  preview: string | null;
  lost: boolean;
  auto: boolean;
}

/** 长截图控制条 + 实时预览 */
export function ScrollCtrl() {
  const [p, setP] = useState<Progress | null>(null);
  const [auto, setAuto] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const un = listen<Progress>("scroll:progress", (e) => {
      setP((old) => ({ ...e.payload, preview: e.payload.preview ?? old?.preview ?? null }));
      // 后端在多次对不上时会停止自动滚动
      setAuto(e.payload.auto);
    });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") invoke("scroll_stop", { cancel: true });
      if (e.key === "Enter") invoke("scroll_stop", { cancel: false });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      un.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  // 预览始终滚到最底部
  useEffect(() => {
    const el = boxRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [p?.preview]);

  const toggleAuto = () => {
    const next = !auto;
    setAuto(next);
    invoke("scroll_auto", { on: next });
  };

  return (
    <div className="scroll-ctrl">
      <div className="sc-title">长截图</div>
      <div className="sc-tip">
        {auto ? "自动滚动中，到底后自动完成" : "在选区内滚动鼠标滚轮"}
        <div className="sc-keys">Enter 完成 · Esc 取消</div>
      </div>
      <div className="sc-preview" ref={boxRef}>
        {p?.preview ? <img src={shotUrl(p.preview)} /> : <div className="sc-empty">等待滚动…</div>}
      </div>
      <div className="sc-status">
        {p ? `已拼接 ${p.width} × ${p.height}` : " "}
        {p?.lost && <div className="sc-warn">画面没对上，请往回滚一点再继续</div>}
      </div>
      <button className={`sc-auto ${auto ? "active" : ""}`} onClick={toggleAuto}>
        {auto ? <Pause size={15} /> : <Play size={15} />}
        {auto ? "停止自动滚动" : "自动滚动"}
      </button>
      <div className="sc-buttons">
        <button onClick={() => invoke("scroll_stop", { cancel: true })}>取消</button>
        <button className="primary" onClick={() => invoke("scroll_stop", { cancel: false })}>
          完成
        </button>
      </div>
    </div>
  );
}
