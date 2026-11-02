import { invoke } from "@tauri-apps/api/core";
import { applyPalette, GIFEncoder, quantize } from "gifenc";
import { Copy, FileImage, FolderOpen, Save } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { errorText, shotUrl } from "../lib/ipc";

interface FileInfo {
  id: string;
  path: string;
  name: string;
  size: number;
}

const sizeText = (n: number) => (n > 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);

/** 录屏完成窗口 */
export function VideoView({ id }: { id: string }) {
  const [info, setInfo] = useState<FileInfo | null>(null);
  const [gif, setGif] = useState<FileInfo | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [gifWidth, setGifWidth] = useState(640);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const cancelRef = useRef(false);

  useEffect(() => {
    invoke<FileInfo>("file_info", { id })
      .then(setInfo)
      .catch((e) => setMsg({ ok: false, text: errorText(e) }));
  }, [id]);

  const say = (text: string, ok = true) => {
    setMsg({ ok, text });
    setTimeout(() => setMsg(null), 2500);
  };
  const run = async (fn: () => Promise<unknown>, done?: string) => {
    try {
      await fn();
      if (done) say(done);
    } catch (e) {
      say(errorText(e), false);
    }
  };

  /** 在页面里逐帧解码视频，用 gifenc 编码为 GIF */
  const makeGif = async () => {
    const src = videoRef.current;
    if (!src || !info) return;
    cancelRef.current = false;
    setGif(null);
    setProgress(0);
    try {
      const v = document.createElement("video");
      v.crossOrigin = "anonymous";
      v.muted = true;
      v.preload = "auto";
      v.src = shotUrl(`file/${id}`);
      await new Promise<void>((res, rej) => {
        v.onloadeddata = () => res();
        v.onerror = () => rej(new Error("视频加载失败"));
      });
      const fps = 10;
      const w = Math.min(gifWidth, v.videoWidth) & ~1;
      const h = Math.round((v.videoHeight * w) / v.videoWidth) & ~1;
      const canvas = document.createElement("canvas");
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d", { willReadFrequently: true })!;
      const enc = GIFEncoder();
      const total = Math.max(1, Math.floor(v.duration * fps));
      for (let i = 0; i < total; i++) {
        if (cancelRef.current) throw new Error("已取消");
        v.currentTime = Math.min(v.duration - 0.001, i / fps);
        await new Promise<void>((res) => (v.onseeked = () => res()));
        ctx.drawImage(v, 0, 0, w, h);
        const { data } = ctx.getImageData(0, 0, w, h);
        const palette = quantize(data, 256);
        enc.writeFrame(applyPalette(data, palette), w, h, { palette, delay: 1000 / fps });
        setProgress((i + 1) / total);
      }
      enc.finish();
      const bytes = enc.bytes();
      const gifId = await invoke<string>("gif_save", bytes, { headers: { "x-source": id } });
      setGif(await invoke<FileInfo>("file_info", { id: gifId }));
    } catch (e) {
      say(errorText(e), false);
    } finally {
      setProgress(null);
    }
  };

  if (!info) return <div className="video-root">{msg && <div className="video-msg bad">{msg.text}</div>}</div>;
  const duration = videoRef.current?.duration ?? 0;
  return (
    <div className="video-root">
      <div className="video-player">
        <video ref={videoRef} src={shotUrl(`file/${id}`)} crossOrigin="anonymous" controls autoPlay muted />
      </div>
      <div className="video-bar">
        <div className="video-file" title={info.path}>
          <div className="video-name">{info.name}</div>
          <div className="muted">
            {sizeText(info.size)} · 已保存到 {info.path.replace(/[\\/][^\\/]+$/, "")}
          </div>
        </div>
        <button onClick={() => run(() => invoke("file_copy", { id }), "已复制，可直接粘贴到聊天窗口或文件夹")}>
          <Copy size={15} /> 复制文件
        </button>
        <button onClick={() => run(() => invoke("file_reveal", { id }))}>
          <FolderOpen size={15} /> 打开文件夹
        </button>
        <button onClick={() => run(() => invoke("file_save_as", { id }))}>
          <Save size={15} /> 另存为
        </button>
      </div>
      <div className="video-gif">
        <FileImage size={16} />
        <span>转为 GIF</span>
        <select value={gifWidth} onChange={(e) => setGifWidth(Number(e.target.value))} disabled={progress !== null}>
          <option value={480}>宽 480</option>
          <option value={640}>宽 640</option>
          <option value={960}>宽 960</option>
          <option value={99999}>原始尺寸</option>
        </select>
        <span className="muted">每秒 10 帧{duration > 30 ? "，视频较长，生成的 GIF 会很大" : ""}</span>
        <span className="grow" />
        {progress !== null ? (
          <>
            <div className="usage-bar">
              <div style={{ width: `${Math.round(progress * 100)}%` }} />
            </div>
            <span className="muted">{Math.round(progress * 100)}%</span>
            <button onClick={() => (cancelRef.current = true)}>取消</button>
          </>
        ) : (
          <button className="primary" onClick={makeGif}>
            生成 GIF
          </button>
        )}
      </div>
      {gif && (
        <div className="video-gif done">
          <span className="ok">✓ {gif.name}（{sizeText(gif.size)}）</span>
          <span className="grow" />
          <button onClick={() => run(() => invoke("file_copy", { id: gif.id }), "GIF 已复制")}>复制 GIF</button>
          <button onClick={() => run(() => invoke("file_reveal", { id: gif.id }))}>打开文件夹</button>
        </div>
      )}
      {msg && <div className={`video-msg ${msg.ok ? "ok" : "bad"}`}>{msg.text}</div>}
    </div>
  );
}
