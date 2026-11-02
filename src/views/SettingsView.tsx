import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, type KeyboardEvent } from "react";
import { errorText } from "../lib/ipc";

interface Usage {
  month: string;
  chars: number;
}
interface Settings {
  hotkeyCapture: string;
  hotkeyPin: string;
  saveDir: string;
  imageFormat: string;
  fileNamePrefix: string;
  autostart: boolean;
  translateProvider: string;
  translateTarget: string;
  azureRegion: string;
  openaiBaseUrl: string;
  openaiModel: string;
  quota: Record<string, number>;
  quotaRatio: number;
  usage: Record<string, Usage>;
}
type Secrets = Record<string, Record<string, string>>;
type Msg = { ok: boolean; text: string } | null;

interface Provider {
  id: string;
  name: string;
  fields: { key: string; label: string; secret?: boolean; placeholder?: string }[];
  apply?: string;
  note: string;
}

const PROVIDERS: Provider[] = [
  {
    id: "tencent",
    name: "腾讯云机器翻译",
    fields: [
      { key: "secretId", label: "SecretId" },
      { key: "secretKey", label: "SecretKey", secret: true },
    ],
    apply: "https://console.cloud.tencent.com/cam/capi",
    note: "每月免费 500 万字符。建议在腾讯云控制台关闭「后付费」，免费额度用完后会停止服务，不会扣费。",
  },
  {
    id: "baidu",
    name: "百度翻译开放平台",
    fields: [
      { key: "appId", label: "APP ID" },
      { key: "appKey", label: "密钥", secret: true },
    ],
    apply: "https://fanyi-api.baidu.com/manage/developer",
    note: "个人认证后切换到「高级版」，每月免费 100 万字符（标准版只有 5 万）。",
  },
  {
    id: "azure",
    name: "微软 Azure Translator",
    fields: [{ key: "key", label: "密钥 (Key)", secret: true }],
    apply: "https://portal.azure.com/#create/Microsoft.CognitiveServicesTextTranslation",
    note: "选择 F0 免费层，每月 200 万字符，超出后直接停止服务，不会扣费。",
  },
  {
    id: "openai",
    name: "OpenAI 兼容接口（大模型）",
    fields: [{ key: "key", label: "API Key", secret: true, placeholder: "本机 Ollama 可留空" }],
    note: "可接 DeepSeek、通义千问、Kimi 等支持 OpenAI 格式的大模型，也可以接本机的 Ollama（完全离线）。翻译质量最好，按各平台价格计费。",
  },
];

/** OpenAI 兼容接口的常用预设 */
const OPENAI_PRESETS = [
  { name: "DeepSeek", url: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { name: "通义千问", url: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  { name: "本机 Ollama", url: "http://localhost:11434/v1", model: "qwen2.5:7b" },
];

/** 把按键转换为快捷键字符串，例如 Alt+Shift+A */
function toAccelerator(e: KeyboardEvent): string | null {
  const code = e.code;
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit[0-9]$/.test(code)) key = code.slice(5);
  else if (/^F([1-9]|1[0-2])$/.test(code)) key = code;
  else if (code === "PrintScreen" || code === "Space") key = code;
  if (!key) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift"].filter(Boolean) as string[];
  if (!mods.length && !/^F\d+$/.test(key) && key !== "PrintScreen") return null;
  return [...mods, key].join("+");
}

function HotkeyInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [recording, setRecording] = useState(false);
  return (
    <input
      className={`hotkey ${recording ? "recording" : ""}`}
      readOnly
      value={recording ? "请按下新的快捷键…" : value || "未设置"}
      onFocus={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={(e) => {
        e.preventDefault();
        if (e.key === "Escape") return (e.target as HTMLInputElement).blur();
        if (e.key === "Backspace" || e.key === "Delete") {
          onChange("");
          return (e.target as HTMLInputElement).blur();
        }
        const acc = toAccelerator(e);
        if (acc) {
          onChange(acc);
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}

export function SettingsView() {
  const [s, setS] = useState<Settings | null>(null);
  const [month, setMonth] = useState("");
  const [secrets, setSecrets] = useState<Secrets>({});
  const [msg, setMsg] = useState<Msg>(null);

  const load = async () => {
    const r = await invoke<{ settings: Settings; month: string }>("get_settings");
    setS(r.settings);
    setMonth(r.month);
    const all: Secrets = {};
    for (const p of PROVIDERS) all[p.id] = (await invoke<Record<string, string>>("get_secret", { provider: p.id })) ?? {};
    setSecrets(all);
  };
  useEffect(() => {
    load();
  }, []);

  if (!s) return null;
  // 函数式更新：连续调用多次也不会互相覆盖
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((prev) => (prev ? { ...prev, [k]: v } : prev));

  const persist = async () => {
    for (const p of PROVIDERS) await invoke("set_secret", { provider: p.id, value: secrets[p.id] ?? {} });
    return invoke<string[]>("save_settings", { value: s });
  };

  const save = async () => {
    try {
      const errors = await persist();
      setMsg(errors.length ? { ok: false, text: errors.join("；") } : { ok: true, text: "已保存" });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    }
  };

  const pickDir = async () => {
    const d = await invoke<string | null>("pick_folder", { current: s.saveDir });
    if (d) set("saveDir", d);
  };

  return (
    <div className="settings">
      <h2>快捷键</h2>
      <div className="row">
        <label>截图</label>
        <HotkeyInput value={s.hotkeyCapture} onChange={(v) => set("hotkeyCapture", v)} />
      </div>
      <div className="row">
        <label>贴剪贴板图片</label>
        <HotkeyInput value={s.hotkeyPin} onChange={(v) => set("hotkeyPin", v)} />
      </div>
      <div className="hint">点击输入框后按下组合键；按 Backspace 清除。</div>

      <h2>保存</h2>
      <div className="row">
        <label>保存目录</label>
        <input className="grow" value={s.saveDir} onChange={(e) => set("saveDir", e.target.value)} />
        <button onClick={pickDir}>选择…</button>
      </div>
      <div className="row">
        <label>默认格式</label>
        <select value={s.imageFormat} onChange={(e) => set("imageFormat", e.target.value)}>
          <option value="png">PNG（无损）</option>
          <option value="jpg">JPG（体积小）</option>
        </select>
      </div>
      <div className="row">
        <label>文件名前缀</label>
        <input value={s.fileNamePrefix} onChange={(e) => set("fileNamePrefix", e.target.value)} />
        <span className="muted">例：{s.fileNamePrefix}_20261102_103000.{s.imageFormat}</span>
      </div>

      <h2>启动</h2>
      <div className="row">
        <label>开机自动启动</label>
        <input type="checkbox" checked={s.autostart} onChange={(e) => set("autostart", e.target.checked)} />
      </div>

      <h2>翻译</h2>
      <div className="row">
        <label>目标语言</label>
        <select value={s.translateTarget} onChange={(e) => set("translateTarget", e.target.value)}>
          <option value="auto">自动（原文中文→英文，其他→中文）</option>
          <option value="zh">总是译成中文</option>
          <option value="en">总是译成英文</option>
        </select>
      </div>

      <OnlineBox s={s} set={set} month={month} secrets={secrets} setSecrets={setSecrets} persist={persist} setMsg={setMsg} reload={load} />

      <h2>关于</h2>
      <div className="about">
        <div>Screenshot 1.0.0</div>
        <div className="muted">文字识别：PaddleOCR PP-OCRv5（Apache-2.0）</div>
      </div>

      <div className="settings-footer">
        {msg && <span className={msg.ok ? "ok" : "bad"}>{msg.text}</span>}
        <span className="grow" />
        <button className="primary" onClick={save}>
          保存设置
        </button>
      </div>
    </div>
  );
}

interface BoxProps {
  s: Settings;
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
  persist: () => Promise<string[]>;
  setMsg: (m: Msg) => void;
}

function OnlineBox({
  s,
  set,
  month,
  secrets,
  setSecrets,
  persist,
  setMsg,
  reload,
}: BoxProps & { month: string; secrets: Secrets; setSecrets: (v: Secrets) => void; reload: () => Promise<void> }) {
  const [testing, setTesting] = useState(false);
  const provider = PROVIDERS.find((p) => p.id === s.translateProvider) ?? PROVIDERS[0];
  const usage = s.usage[provider.id];
  const used = usage && usage.month === month ? usage.chars : 0;
  const quota = s.quota[provider.id] ?? 0;

  const test = async () => {
    setTesting(true);
    setMsg(null);
    try {
      await persist();
      const r = await invoke<string>("test_translate", { provider: provider.id });
      setMsg({ ok: true, text: `连接成功：${r}` });
      await reload();
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setTesting(false);
    }
  };

  return (
    <>
      <div className="row">
        <label>翻译服务</label>
        <select value={s.translateProvider} onChange={(e) => set("translateProvider", e.target.value)}>
          {PROVIDERS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </div>
      <div className="provider-box">
        <div className="hint">{provider.note}</div>
        {provider.apply && (
          <div className="hint">
            申请地址：<span className="selectable">{provider.apply}</span>
          </div>
        )}
        {provider.id === "openai" && (
          <>
            <div className="row">
              <label>常用预设</label>
              {OPENAI_PRESETS.map((p) => (
                <button
                  key={p.name}
                  onClick={() => {
                    set("openaiBaseUrl", p.url);
                    set("openaiModel", p.model);
                  }}
                >
                  {p.name}
                </button>
              ))}
            </div>
            <div className="row">
              <label>接口地址</label>
              <input className="grow" value={s.openaiBaseUrl} placeholder="https://api.deepseek.com/v1" onChange={(e) => set("openaiBaseUrl", e.target.value)} />
            </div>
            <div className="row">
              <label>模型名</label>
              <input className="grow" value={s.openaiModel} placeholder="deepseek-chat" onChange={(e) => set("openaiModel", e.target.value)} />
            </div>
          </>
        )}
        {provider.fields.map((f) => (
          <div className="row" key={f.key}>
            <label>{f.label}</label>
            <input
              className="grow"
              type={f.secret ? "password" : "text"}
              placeholder={f.placeholder}
              value={secrets[provider.id]?.[f.key] ?? ""}
              onChange={(e) => setSecrets({ ...secrets, [provider.id]: { ...secrets[provider.id], [f.key]: e.target.value } })}
            />
          </div>
        ))}
        {provider.id === "azure" && (
          <div className="row">
            <label>区域 (Region)</label>
            <input value={s.azureRegion} placeholder="例如 eastasia，全局资源可留空" onChange={(e) => set("azureRegion", e.target.value)} />
          </div>
        )}
        <div className="row">
          <label>每月额度</label>
          <input
            type="number"
            value={quota}
            onChange={(e) => set("quota", { ...s.quota, [provider.id]: Math.max(0, Number(e.target.value) || 0) })}
          />
          <span className="muted">字符，0 表示不限制</span>
        </div>
        <div className="row">
          <label>用到多少暂停</label>
          <input
            type="number"
            min={10}
            max={100}
            value={Math.round(s.quotaRatio * 100)}
            onChange={(e) => set("quotaRatio", Math.min(100, Math.max(10, Number(e.target.value) || 95)) / 100)}
          />
          <span className="muted">%（所有服务共用）</span>
        </div>
        <div className="row">
          <label>本月已用</label>
          <div className="usage">
            {quota > 0 && (
              <div className="usage-bar">
                <div style={{ width: `${Math.min(100, (used / quota) * 100)}%` }} />
              </div>
            )}
            <span className="muted">
              {used.toLocaleString()}
              {quota > 0 ? ` / ${quota.toLocaleString()}` : ""} 字符（{month}）
            </span>
          </div>
        </div>
        <div className="row">
          <label />
          <button onClick={test} disabled={testing}>
            {testing ? "测试中…" : "测试连接"}
          </button>
        </div>
      </div>
    </>
  );
}
