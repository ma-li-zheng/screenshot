import { invoke } from "@tauri-apps/api/core";
import { Mic, MicOff, Pause, Play, Square, Volume2, VolumeX, X } from "lucide-react";
import { useEffect, useState } from "react";

/** 录制区域的红色边框（透明、鼠标穿透、不会被录进视频） */
export function RecordFrame() {
  return <div className="record-frame" />;
}

interface Status {
  recording: boolean;
  paused: boolean;
  seconds: number;
  hasSys: boolean;
  hasMic: boolean;
  muteSys: boolean;
  muteMic: boolean;
  error: string | null;
}

const fmt = (s: number) => {
  const t = Math.floor(s);
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const sec = t % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
};

/** 录制控制条：时长、暂停、声音开关、停止 */
export function RecordCtrl() {
  const [st, setSt] = useState<Status | null>(null);
  const [stopping, setStopping] = useState(false);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      const s = await invoke<Status>("record_status");
      if (alive && s) setSt(s);
    };
    tick();
    const id = setInterval(tick, 250);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  if (!st) return <div className="record-ctrl" />;
  const stop = () => {
    setStopping(true);
    invoke("record_stop", { cancel: false });
  };
  const cancel = () => {
    if (confirm("放弃这段录制？视频不会保存。")) invoke("record_stop", { cancel: true });
  };
  const toggleMute = (source: "sys" | "mic", mute: boolean) => {
    invoke("record_mute", { source, mute });
    setSt({ ...st, [source === "sys" ? "muteSys" : "muteMic"]: mute });
  };

  return (
    <div className="record-ctrl">
      <div className="rc-time" data-tauri-drag-region title="按住拖动">
        <span className={`rc-dot ${st.paused ? "paused" : ""}`} data-tauri-drag-region />
        <span data-tauri-drag-region>{stopping ? "正在保存…" : fmt(st.seconds)}</span>
      </div>
      <button
        title={st.paused ? "继续" : "暂停"}
        disabled={stopping}
        onClick={() => {
          invoke("record_pause", { paused: !st.paused });
          setSt({ ...st, paused: !st.paused });
        }}
      >
        {st.paused ? <Play size={16} /> : <Pause size={16} />}
      </button>
      {st.hasSys && (
        <button
          className={st.muteSys ? "off" : ""}
          title={st.muteSys ? "系统声音：已静音" : "系统声音：录制中"}
          disabled={stopping}
          onClick={() => toggleMute("sys", !st.muteSys)}
        >
          {st.muteSys ? <VolumeX size={16} /> : <Volume2 size={16} />}
        </button>
      )}
      {st.hasMic && (
        <button
          className={st.muteMic ? "off" : ""}
          title={st.muteMic ? "麦克风：已静音" : "麦克风：录制中"}
          disabled={stopping}
          onClick={() => toggleMute("mic", !st.muteMic)}
        >
          {st.muteMic ? <MicOff size={16} /> : <Mic size={16} />}
        </button>
      )}
      <span className="grow" data-tauri-drag-region />
      <button className="rc-stop" title="停止并保存" disabled={stopping} onClick={stop}>
        <Square size={13} fill="currentColor" />
        <span>停止</span>
      </button>
      <button title="放弃录制" disabled={stopping} onClick={cancel}>
        <X size={16} />
      </button>
      {st.error && <div className="rc-error" title={st.error}>!</div>}
    </div>
  );
}
