import { useEffect, useRef } from "react";
import { commitText } from "./drawing";
import { useEditor } from "./store";
import { FONT_FAMILY } from "./types";

/** 文字输入框，叠在舞台上对应位置 */
export function TextEditor({ offsetX = 0, offsetY = 0 }: { offsetX?: number; offsetY?: number }) {
  const t = useEditor((s) => s.textEdit);
  const set = useEditor((s) => s.setTextEdit);
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (t) setTimeout(() => ref.current?.focus(), 0);
  }, [t?.id, t?.x, t?.y]);

  if (!t) return null;
  return (
    <textarea
      ref={ref}
      className="text-editor"
      value={t.text}
      spellCheck={false}
      style={{
        left: t.x + offsetX - 3,
        top: t.y + offsetY - 3,
        color: t.color,
        fontSize: t.size,
        fontFamily: FONT_FAMILY,
        lineHeight: 1.25,
      }}
      onChange={(e) => set({ ...t, text: e.target.value })}
      onBlur={() => commitText()}
      onMouseDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape" || (e.key === "Enter" && (e.ctrlKey || e.metaKey))) {
          e.preventDefault();
          commitText();
        }
      }}
    />
  );
}
