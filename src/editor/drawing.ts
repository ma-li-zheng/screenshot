import type Konva from "konva";
import { useEditor } from "./store";
import type { Rect } from "./types";
import { clamp, newId } from "./types";

/** 点击目标若是已有批注，返回它的 id */
export function annoIdOf(target: Konva.Node | null): string | null {
  let n: Konva.Node | null = target;
  while (n) {
    if (n.name() === "anno") return n.id();
    n = n.getParent();
  }
  return null;
}

/**
 * 批注绘制：返回是否消费了本次按下。
 * region 为可绘制区域（舞台坐标）。
 */
export function beginDraw(x: number, y: number, target: Konva.Node | null, region: Rect): boolean {
  const st = useEditor.getState();
  if (st.textEdit) {
    commitText();
    return true;
  }
  const hitId = annoIdOf(target);
  if (hitId) {
    st.select(hitId);
    return true;
  }
  st.select(null);
  const tool = st.tool;
  if (!tool) return false;
  if (x < region.x || y < region.y || x > region.x + region.w || y > region.y + region.h) return false;

  const size = st.size();
  const color = st.color;
  if (tool === "text") {
    st.setTextEdit({ x, y: y - size * 0.6, text: "", color, size });
    return true;
  }
  if (tool === "number") {
    const n = st.shapes.filter((s) => s.type === "number").reduce((m, s) => Math.max(m, s.n ?? 0), 0) + 1;
    st.add({ id: newId(), type: "number", color, size, x, y, n });
    return true;
  }
  const base = { id: newId(), type: tool, color, size, x: 0, y: 0 };
  if (tool === "rect" || tool === "ellipse") st.setDraft({ ...base, x, y, w: 0, h: 0, points: [x, y] });
  else if (tool === "arrow" || tool === "line") st.setDraft({ ...base, points: [x, y, x, y] });
  else st.setDraft({ ...base, points: [x, y] });
  return true;
}

export function moveDraw(x: number, y: number, region: Rect, shift: boolean) {
  const st = useEditor.getState();
  const d = st.draft;
  if (!d) return;
  x = clamp(x, region.x, region.x + region.w);
  y = clamp(y, region.y, region.y + region.h);
  const pts = d.points ?? [];
  if (d.type === "rect" || d.type === "ellipse") {
    const [ox, oy] = pts;
    let w = x - ox;
    let h = y - oy;
    if (shift) {
      const m = Math.max(Math.abs(w), Math.abs(h));
      w = Math.sign(w || 1) * m;
      h = Math.sign(h || 1) * m;
    }
    st.setDraft({ ...d, x: Math.min(ox, ox + w), y: Math.min(oy, oy + h), w: Math.abs(w), h: Math.abs(h) });
  } else if (d.type === "arrow" || d.type === "line") {
    if (shift) {
      // 按住 Shift 约束到 45° 的倍数
      const dx = x - pts[0];
      const dy = y - pts[1];
      const ang = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
      const len = Math.hypot(dx, dy);
      x = pts[0] + Math.cos(ang) * len;
      y = pts[1] + Math.sin(ang) * len;
    }
    st.setDraft({ ...d, points: [pts[0], pts[1], x, y] });
  } else {
    const lx = pts[pts.length - 2];
    const ly = pts[pts.length - 1];
    if (Math.hypot(x - lx, y - ly) < 1.5) return;
    st.setDraft({ ...d, points: [...pts, x, y] });
  }
}

export function endDraw() {
  const st = useEditor.getState();
  const d = st.draft;
  if (!d) return;
  st.setDraft(null);
  const pts = d.points ?? [];
  let ok = true;
  if (d.type === "rect" || d.type === "ellipse") ok = (d.w ?? 0) > 3 && (d.h ?? 0) > 3;
  else if (d.type === "arrow" || d.type === "line") ok = Math.hypot(pts[2] - pts[0], pts[3] - pts[1]) > 4;
  if (!ok) return;
  if (d.type === "rect" || d.type === "ellipse") st.add({ ...d, points: undefined });
  else st.add(d);
}

export function commitText() {
  const st = useEditor.getState();
  const t = st.textEdit;
  if (!t) return;
  st.setTextEdit(null);
  const text = t.text.replace(/\s+$/, "");
  if (t.id) {
    if (text) st.update(t.id, { text, color: t.color, size: t.size });
    else st.remove(t.id);
  } else if (text) {
    st.add({ id: newId(), type: "text", color: t.color, size: t.size, x: t.x, y: t.y, text });
  }
}

/** 编辑器通用快捷键；返回 true 表示已处理 */
export function editorKey(e: KeyboardEvent): boolean {
  const st = useEditor.getState();
  if (st.textEdit) return false;
  const ctrl = e.ctrlKey || e.metaKey;
  if (ctrl && e.key.toLowerCase() === "z") {
    if (e.shiftKey) st.redo();
    else st.undo();
    return true;
  }
  if (ctrl && e.key.toLowerCase() === "y") {
    st.redo();
    return true;
  }
  if ((e.key === "Delete" || e.key === "Backspace") && st.selectedId) {
    st.remove(st.selectedId);
    return true;
  }
  return false;
}
