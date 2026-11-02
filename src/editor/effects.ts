/** 马赛克、模糊画笔用的底图：基于原始截图预先生成，画笔以 pattern 方式"刷"出来 */
const cache = new WeakMap<HTMLCanvasElement, { mosaic?: HTMLCanvasElement; blur?: HTMLCanvasElement }>();

export function effectCanvas(bg: HTMLCanvasElement, kind: "mosaic" | "blur", dpr: number): HTMLCanvasElement {
  let entry = cache.get(bg);
  if (!entry) {
    entry = {};
    cache.set(bg, entry);
  }
  const hit = entry[kind];
  if (hit) return hit;

  const c = document.createElement("canvas");
  c.width = bg.width;
  c.height = bg.height;
  const ctx = c.getContext("2d")!;
  if (kind === "mosaic") {
    const block = Math.max(4, Math.round(9 * dpr));
    const sw = Math.max(1, Math.ceil(bg.width / block));
    const sh = Math.max(1, Math.ceil(bg.height / block));
    const small = document.createElement("canvas");
    small.width = sw;
    small.height = sh;
    const sctx = small.getContext("2d")!;
    sctx.imageSmoothingEnabled = true;
    sctx.drawImage(bg, 0, 0, sw, sh);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(small, 0, 0, sw * block, sh * block);
  } else {
    ctx.filter = `blur(${Math.round(8 * dpr)}px)`;
    ctx.drawImage(bg, 0, 0);
  }
  entry[kind] = c;
  return c;
}
