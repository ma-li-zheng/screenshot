import { create } from "zustand";
import type { Shape, Tool } from "./types";
import { COLORS, SIZES } from "./types";

export interface TextEdit {
  id?: string;
  x: number;
  y: number;
  text: string;
  color: string;
  size: number;
}

interface EditorState {
  tool: Tool | null;
  color: string;
  sizeIdx: Record<Tool, number>;
  shapes: Shape[];
  past: Shape[][];
  future: Shape[][];
  draft: Shape | null;
  selectedId: string | null;
  textEdit: TextEdit | null;

  setTool: (t: Tool | null) => void;
  setColor: (c: string) => void;
  setSizeIdx: (i: number) => void;
  size: () => number;
  commit: (next: Shape[]) => void;
  add: (s: Shape) => void;
  update: (id: string, patch: Partial<Shape>) => void;
  remove: (id: string) => void;
  undo: () => void;
  redo: () => void;
  setDraft: (s: Shape | null) => void;
  select: (id: string | null) => void;
  setTextEdit: (t: TextEdit | null) => void;
  reset: () => void;
}

const initialSizes = Object.fromEntries(Object.keys(SIZES).map((k) => [k, 1])) as Record<Tool, number>;

export const useEditor = create<EditorState>((set, get) => ({
  tool: null,
  color: COLORS[0],
  sizeIdx: initialSizes,
  shapes: [],
  past: [],
  future: [],
  draft: null,
  selectedId: null,
  textEdit: null,

  setTool: (t) => set({ tool: t, selectedId: null }),
  setColor: (c) => {
    const { selectedId, shapes } = get();
    set({ color: c });
    // 选中图形时，改颜色作用到该图形
    const s = shapes.find((x) => x.id === selectedId);
    if (s && s.type !== "mosaic" && s.type !== "blur") get().update(s.id, { color: c });
  },
  setSizeIdx: (i) => {
    const { tool } = get();
    if (!tool) return;
    set((st) => ({ sizeIdx: { ...st.sizeIdx, [tool]: i } }));
  },
  size: () => {
    const { tool, sizeIdx } = get();
    return tool ? SIZES[tool][sizeIdx[tool]] : 4;
  },
  commit: (next) => set((st) => ({ past: [...st.past, st.shapes].slice(-100), future: [], shapes: next })),
  add: (s) => get().commit([...get().shapes, s]),
  update: (id, patch) => get().commit(get().shapes.map((s) => (s.id === id ? { ...s, ...patch } : s))),
  remove: (id) => {
    get().commit(get().shapes.filter((s) => s.id !== id));
    set({ selectedId: null });
  },
  undo: () =>
    set((st) =>
      st.past.length
        ? { shapes: st.past[st.past.length - 1], past: st.past.slice(0, -1), future: [st.shapes, ...st.future], selectedId: null }
        : {},
    ),
  redo: () =>
    set((st) =>
      st.future.length
        ? { shapes: st.future[0], future: st.future.slice(1), past: [...st.past, st.shapes], selectedId: null }
        : {},
    ),
  setDraft: (s) => set({ draft: s }),
  select: (id) => set({ selectedId: id }),
  setTextEdit: (t) => set({ textEdit: t }),
  reset: () =>
    set({ tool: null, shapes: [], past: [], future: [], draft: null, selectedId: null, textEdit: null }),
}));
