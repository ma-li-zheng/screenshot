import type Konva from "konva";
import { forwardRef } from "react";
import { Arrow, Circle, Ellipse, Group, Layer, Line, Rect, Shape as KShape, Text } from "react-konva";
import { effectCanvas } from "./effects";
import { useEditor } from "./store";
import type { Rect as R, Shape } from "./types";
import { FONT_FAMILY } from "./types";

interface Props {
  bg: HTMLCanvasElement | null;
  dpr: number;
  /** 批注只在该区域内可见 */
  clip: R | null;
}

const SELECT_SHADOW = { shadowColor: "#1677FF", shadowBlur: 8, shadowOpacity: 1 };

export const AnnotationLayer = forwardRef<Konva.Layer, Props>(function AnnotationLayer({ bg, dpr, clip }, ref) {
  const shapes = useEditor((s) => s.shapes);
  const draft = useEditor((s) => s.draft);
  const selectedId = useEditor((s) => s.selectedId);
  const editingId = useEditor((s) => s.textEdit?.id);
  const all = draft ? [...shapes, draft] : shapes;
  return (
    <Layer
      ref={ref}
      clipX={clip?.x}
      clipY={clip?.y}
      clipWidth={clip?.w}
      clipHeight={clip?.h}
    >
      {all.map((s) =>
        s.id === editingId ? null : (
          <ShapeNode key={s.id} s={s} bg={bg} dpr={dpr} selected={s.id === selectedId} />
        ),
      )}
    </Layer>
  );
});

function ShapeNode({ s, bg, dpr, selected }: { s: Shape; bg: HTMLCanvasElement | null; dpr: number; selected: boolean }) {
  const update = useEditor((st) => st.update);
  const common = {
    id: s.id,
    name: "anno",
    x: s.x,
    y: s.y,
    draggable: selected,
    hitStrokeWidth: Math.max(s.size, 12),
    onDragEnd: (e: Konva.KonvaEventObject<DragEvent>) => update(s.id, { x: e.target.x(), y: e.target.y() }),
    ...(selected ? SELECT_SHADOW : {}),
  };
  switch (s.type) {
    case "rect":
      return <Rect {...common} width={s.w} height={s.h} stroke={s.color} strokeWidth={s.size} />;
    case "ellipse":
      return (
        <Ellipse
          {...common}
          x={s.x + (s.w ?? 0) / 2}
          y={s.y + (s.h ?? 0) / 2}
          radiusX={(s.w ?? 0) / 2}
          radiusY={(s.h ?? 0) / 2}
          stroke={s.color}
          strokeWidth={s.size}
          onDragEnd={(e) => update(s.id, { x: e.target.x() - (s.w ?? 0) / 2, y: e.target.y() - (s.h ?? 0) / 2 })}
        />
      );
    case "arrow": {
      const head = s.size * 3 + 6;
      return (
        <Arrow
          {...common}
          points={s.points ?? []}
          stroke={s.color}
          fill={s.color}
          strokeWidth={s.size}
          pointerLength={head}
          pointerWidth={head}
          lineCap="round"
          lineJoin="round"
        />
      );
    }
    case "line":
      return <Line {...common} points={s.points ?? []} stroke={s.color} strokeWidth={s.size} lineCap="round" />;
    case "pen":
      return (
        <Line {...common} points={s.points ?? []} stroke={s.color} strokeWidth={s.size} lineCap="round" lineJoin="round" />
      );
    case "highlighter":
      return (
        <Line
          {...common}
          points={s.points ?? []}
          stroke={s.color}
          strokeWidth={s.size}
          opacity={0.4}
          lineCap="round"
          lineJoin="round"
        />
      );
    case "text":
      return (
        <Text
          {...common}
          text={s.text}
          fontSize={s.size}
          fontFamily={FONT_FAMILY}
          lineHeight={1.25}
          fill={s.color}
          onDblClick={() =>
            useEditor.getState().setTextEdit({ id: s.id, x: s.x, y: s.y, text: s.text ?? "", color: s.color, size: s.size })
          }
        />
      );
    case "number": {
      const r = s.size;
      const light = s.color.toUpperCase() === "#FFFFFF" || s.color.toUpperCase() === "#FADB14";
      return (
        <Group {...common}>
          <Circle radius={r} fill={s.color} stroke="#fff" strokeWidth={1.5} />
          <Text
            text={String(s.n)}
            fontSize={r * 1.2}
            fontStyle="bold"
            fontFamily={FONT_FAMILY}
            fill={light ? "#000" : "#fff"}
            width={r * 2}
            height={r * 2}
            x={-r}
            y={-r}
            align="center"
            verticalAlign="middle"
          />
        </Group>
      );
    }
    case "mosaic":
    case "blur":
      if (!bg) return null;
      return (
        <KShape
          listening={false}
          sceneFunc={(ctx) => {
            const c = (ctx as unknown as { _context: CanvasRenderingContext2D })._context;
            const pts = s.points ?? [];
            if (pts.length < 2) return;
            const pat = c.createPattern(effectCanvas(bg, s.type as "mosaic" | "blur", dpr), "no-repeat");
            if (!pat) return;
            pat.setTransform(new DOMMatrix([1 / dpr, 0, 0, 1 / dpr, 0, 0]));
            c.save();
            c.lineCap = "round";
            c.lineJoin = "round";
            c.lineWidth = s.size;
            c.strokeStyle = pat;
            c.beginPath();
            c.moveTo(pts[0], pts[1]);
            if (pts.length === 2) c.lineTo(pts[0] + 0.01, pts[1]);
            for (let i = 2; i < pts.length; i += 2) c.lineTo(pts[i], pts[i + 1]);
            c.stroke();
            c.restore();
          }}
        />
      );
  }
}
