import {
  Check,
  Circle,
  Download,
  Droplet,
  Grid3x3,
  Highlighter,
  Minus,
  MoveUpRight,
  Pencil,
  Pin,
  Redo2,
  ScanText,
  ScrollText,
  Square,
  Type,
  Undo2,
  Video,
  X,
} from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { useEditor } from "./store";
import type { Tool } from "./types";
import { COLORS, SIZES } from "./types";

export type ToolbarAction = "undo" | "redo" | "long" | "record" | "ocr" | "pin" | "save" | "cancel" | "done";

const TOOLS: { tool: Tool; icon: ReactNode; tip: string }[] = [
  { tool: "rect", icon: <Square size={18} />, tip: "矩形" },
  { tool: "ellipse", icon: <Circle size={18} />, tip: "椭圆" },
  { tool: "arrow", icon: <MoveUpRight size={18} />, tip: "箭头" },
  { tool: "line", icon: <Minus size={18} style={{ transform: "rotate(-45deg)" }} />, tip: "直线" },
  { tool: "pen", icon: <Pencil size={18} />, tip: "画笔" },
  { tool: "highlighter", icon: <Highlighter size={18} />, tip: "荧光笔" },
  { tool: "text", icon: <Type size={18} />, tip: "文字" },
  { tool: "mosaic", icon: <Grid3x3 size={18} />, tip: "马赛克" },
  { tool: "blur", icon: <Droplet size={18} />, tip: "模糊" },
  { tool: "number", icon: <span className="num-icon">1</span>, tip: "序号" },
];

export const TOOLBAR_H = 40;
export const PROPS_H = 36;

interface Props {
  style?: CSSProperties;
  /** capture 模式有长截图和取消，image 模式（长图编辑）没有长截图 */
  mode: "capture" | "image";
  onAction: (a: ToolbarAction) => void;
  propsBelow?: boolean;
}

export function Toolbar({ style, mode, onAction, propsBelow = true }: Props) {
  const tool = useEditor((s) => s.tool);
  const setTool = useEditor((s) => s.setTool);
  const canUndo = useEditor((s) => s.past.length > 0);
  const canRedo = useEditor((s) => s.future.length > 0);

  const bar = (
    <div className="toolbar" onMouseDown={(e) => e.stopPropagation()}>
      {TOOLS.map((t) => (
        <button
          key={t.tool}
          className={tool === t.tool ? "active" : ""}
          title={t.tip}
          onClick={() => setTool(tool === t.tool ? null : t.tool)}
        >
          {t.icon}
        </button>
      ))}
      <span className="sep" />
      <button title="撤销 (Ctrl+Z)" disabled={!canUndo} onClick={() => onAction("undo")}>
        <Undo2 size={18} />
      </button>
      <button title="重做 (Ctrl+Y)" disabled={!canRedo} onClick={() => onAction("redo")}>
        <Redo2 size={18} />
      </button>
      <span className="sep" />
      {mode === "capture" && (
        <>
          <button title="长截图" onClick={() => onAction("long")}>
            <ScrollText size={18} />
          </button>
          <button title="录屏" onClick={() => onAction("record")}>
            <Video size={18} />
          </button>
        </>
      )}
      <button title="提取文字（识别后可翻译）" onClick={() => onAction("ocr")}>
        <ScanText size={18} />
      </button>
      <button title="钉在屏幕上" onClick={() => onAction("pin")}>
        <Pin size={18} />
      </button>
      <button title="保存 (Ctrl+S)" onClick={() => onAction("save")}>
        <Download size={18} />
      </button>
      <span className="sep" />
      <button title={mode === "capture" ? "退出截图 (Esc)" : "关闭"} className="danger" onClick={() => onAction("cancel")}>
        <X size={18} />
      </button>
      <button title="完成，复制到剪贴板 (Enter)" className="primary" onClick={() => onAction("done")}>
        <Check size={18} />
        <span>完成</span>
      </button>
    </div>
  );
  const props = tool ? <PropsBar /> : null;
  return (
    <div className="toolbar-wrap" style={style}>
      {!propsBelow && props}
      {bar}
      {propsBelow && props}
    </div>
  );
}

function PropsBar() {
  const tool = useEditor((s) => s.tool)!;
  const color = useEditor((s) => s.color);
  const sizeIdx = useEditor((s) => s.sizeIdx[tool]);
  const setColor = useEditor((s) => s.setColor);
  const setSizeIdx = useEditor((s) => s.setSizeIdx);
  const noColor = tool === "mosaic" || tool === "blur";
  return (
    <div className="propsbar" onMouseDown={(e) => e.stopPropagation()}>
      {SIZES[tool].map((sz, i) => (
        <button key={i} className={`size ${i === sizeIdx ? "active" : ""}`} title={tool === "text" ? `${sz}px` : undefined} onClick={() => setSizeIdx(i)}>
          {tool === "text" ? <span style={{ fontSize: 11 + i * 3 }}>A</span> : <span className="dot" style={{ width: 4 + i * 4, height: 4 + i * 4 }} />}
        </button>
      ))}
      {!noColor && (
        <>
          <span className="sep" />
          {COLORS.map((c) => (
            <button key={c} className={`color ${c === color ? "active" : ""}`} style={{ background: c }} onClick={() => setColor(c)} />
          ))}
          <label className="color custom" title="自定义颜色">
            <input type="color" value={color} onChange={(e) => setColor(e.target.value.toUpperCase())} />
          </label>
        </>
      )}
    </div>
  );
}
