export type Tool =
  | "rect"
  | "ellipse"
  | "arrow"
  | "line"
  | "pen"
  | "highlighter"
  | "text"
  | "mosaic"
  | "blur"
  | "number";

export interface Shape {
  id: string;
  type: Tool;
  color: string;
  size: number;
  /** 平移量；矩形/椭圆/文字/序号的左上角或中心 */
  x: number;
  y: number;
  w?: number;
  h?: number;
  points?: number[];
  text?: string;
  n?: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const COLORS = [
  "#F5222D",
  "#FA8C16",
  "#FADB14",
  "#52C41A",
  "#1677FF",
  "#722ED1",
  "#000000",
  "#FFFFFF",
];

/** 每个工具 3 档粗细（文字为字号，序号为半径） */
export const SIZES: Record<Tool, number[]> = {
  rect: [2, 4, 6],
  ellipse: [2, 4, 6],
  arrow: [2, 4, 6],
  line: [2, 4, 6],
  pen: [2, 4, 6],
  highlighter: [12, 18, 26],
  text: [14, 18, 26],
  mosaic: [12, 20, 32],
  blur: [16, 26, 40],
  number: [10, 13, 17],
};

export const FONT_FAMILY = '"Microsoft YaHei", "PingFang SC", sans-serif';

export function normRect(x1: number, y1: number, x2: number, y2: number): Rect {
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1) };
}

export function clamp(v: number, lo: number, hi: number) {
  return Math.max(lo, Math.min(hi, v));
}

let seq = 0;
export const newId = () => `s${Date.now().toString(36)}${(seq++).toString(36)}`;
