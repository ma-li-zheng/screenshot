//! 长截图拼接：比较相邻两帧，求出滚动距离，只追加新出现的内容。
//! - 每帧按竖条切分，固定不动的竖条（侧边栏、目录、悬浮按钮）不参与对齐
//! - 页面顶部、底部固定不动的区域（吸顶标题栏、底栏）只保留一份
use image::RgbaImage;

pub enum Push {
    /// 内容没有变化
    Same,
    /// 向下滚动并追加了新内容
    Moved,
    /// 找不到重叠（滚动太快或画面变化太大），本帧被丢弃
    Lost,
}

/// 每帧切成的竖条数
const STRIPS: usize = 16;

struct Frame {
    img: RgbaImage,
    strips: usize,
    /// 第 y 行第 s 条的哈希：cells[y * strips + s]
    cells: Vec<u64>,
    /// 该格是否有内容（纯色格和任何格都"匹配"，不参与评分）
    info: Vec<bool>,
}

impl Frame {
    fn new(img: RgbaImage) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        // 右侧滚动条会随滚动变化，计算哈希时排除
        let right = w.saturating_sub((w / 12).clamp(0, 24)).max(1);
        let sw = right.div_ceil(STRIPS).max(1);
        let strips = right.div_ceil(sw);
        let raw = img.as_raw();
        let mut cells = Vec::with_capacity(h * strips);
        let mut info = Vec::with_capacity(h * strips);
        for y in 0..h {
            for s in 0..strips {
                let x0 = s * sw;
                let x1 = ((s + 1) * sw).min(right);
                let seg = &raw[(y * w + x0) * 4..(y * w + x1) * 4];
                // FNV-1a
                let mut hsh: u64 = 0xcbf29ce484222325;
                for b in seg {
                    hsh ^= *b as u64;
                    hsh = hsh.wrapping_mul(0x100000001b3);
                }
                cells.push(hsh);
                let (r0, g0, b0) = (seg[0] as i32, seg[1] as i32, seg[2] as i32);
                info.push(seg.chunks_exact(4).any(|p| {
                    (p[0] as i32 - r0).abs() + (p[1] as i32 - g0).abs() + (p[2] as i32 - b0).abs() > 6
                }));
            }
        }
        Self {
            img,
            strips,
            cells,
            info,
        }
    }

    fn rows(&self) -> usize {
        self.img.height() as usize
    }
}

pub struct Stitcher {
    first: Option<Frame>,
    prev: Option<Frame>,
    /// 固定顶栏、底栏行数，首次检测到滚动时确定
    bands: Option<(usize, usize)>,
    /// 中间滚动区域累积的像素（RGBA 行）
    body: Vec<u8>,
    body_rows: usize,
}

impl Stitcher {
    pub fn new() -> Self {
        Self {
            first: None,
            prev: None,
            bands: None,
            body: Vec::new(),
            body_rows: 0,
        }
    }

    pub fn height(&self) -> usize {
        match (&self.first, self.bands) {
            (Some(_), Some((t, b))) => t + self.body_rows + b,
            (Some(f), None) => f.rows(),
            _ => 0,
        }
    }

    pub fn push(&mut self, img: RgbaImage) -> Push {
        let cur = Frame::new(img);
        let Some(prev) = &self.prev else {
            self.first = Some(Frame::new(cur.img.clone()));
            self.prev = Some(cur);
            return Push::Same;
        };
        if prev.img.dimensions() != cur.img.dimensions() || prev.cells == cur.cells {
            return Push::Same;
        }
        let h = cur.rows();
        let w = cur.img.width() as usize;
        let st = cur.strips;

        // 两帧同一位置几乎完全相同的竖条视为固定区域，不参与对齐
        let moving: Vec<usize> = (0..st)
            .filter(|&s| {
                let same = (0..h).filter(|&y| prev.cells[y * st + s] == cur.cells[y * st + s]).count();
                (same as f64) < h as f64 * 0.97
            })
            .collect();
        if moving.is_empty() {
            return Push::Same;
        }
        let row_equal = |y: usize| moving.iter().all(|&s| prev.cells[y * st + s] == cur.cells[y * st + s]);

        let (top, bottom) = match self.bands {
            Some(b) => b,
            None => {
                let max_band = h / 3;
                let top = (0..h).take_while(|&y| row_equal(y)).count().min(max_band);
                let bottom = (0..h).take_while(|&i| row_equal(h - 1 - i)).count().min(max_band);
                (top, bottom)
            }
        };
        let n = h - bottom - top;
        if n < 8 {
            return Push::Lost;
        }

        // 找使「上一帧中间区域下半部分」与「当前帧上半部分」重合最多的 dy。
        // 逐格比较：跨越固定区域边界的竖条只会贡献少量不匹配，不影响整体判断
        let cell = |f: &Frame, row: usize, s: usize| (f.cells[(top + row) * st + s], f.info[(top + row) * st + s]);
        let mut best: Option<(usize, f64)> = None;
        let min_overlap = (n / 8).max(8);
        for dy in 1..n.saturating_sub(min_overlap) {
            let overlap = n - dy;
            let budget = overlap * moving.len() / 4 + 4;
            let (mut hit, mut miss) = (0usize, 0usize);
            'rows: for i in 0..overlap {
                for &s in &moving {
                    let (pc, pinf) = cell(prev, dy + i, s);
                    let (cc, cinf) = cell(&cur, i, s);
                    if !(pinf || cinf) {
                        continue;
                    }
                    if pc == cc {
                        hit += 1;
                    } else {
                        miss += 1;
                        if miss > budget {
                            break 'rows;
                        }
                    }
                }
            }
            if hit < 6 {
                continue;
            }
            let score = hit as f64 / (hit + miss) as f64;
            if score >= 0.8 && best.map_or(true, |(_, s)| score > s + 1e-9) {
                best = Some((dy, score));
                if score > 0.995 {
                    break;
                }
            }
        }
        let Some((dy, _)) = best else {
            return Push::Lost;
        };

        if self.bands.is_none() {
            self.bands = Some((top, bottom));
            // 用第一帧的中间区域初始化正文
            let f = &self.first.as_ref().unwrap().img;
            self.body
                .extend_from_slice(&f.as_raw()[top * w * 4..(h - bottom) * w * 4]);
            self.body_rows = h - bottom - top;
        }
        // 当前帧中间区域的最后 dy 行是新内容
        let from = h - bottom - dy;
        self.body
            .extend_from_slice(&cur.img.as_raw()[from * w * 4..(h - bottom) * w * 4]);
        self.body_rows += dy;
        self.prev = Some(cur);
        Push::Moved
    }

    /// 拼接结果的最后 max_rows 行（用于实时预览，避免每次复制整张长图）
    pub fn tail(&self, max_rows: usize) -> Option<RgbaImage> {
        let first = self.first.as_ref()?;
        let Some((_, bottom)) = self.bands else {
            return Some(first.img.clone());
        };
        let last = &self.prev.as_ref()?.img;
        let (w, h) = (first.img.width() as usize, first.img.height() as usize);
        let body_take = max_rows.saturating_sub(bottom).min(self.body_rows);
        let mut out = Vec::with_capacity((body_take + bottom) * w * 4);
        out.extend_from_slice(&self.body[(self.body_rows - body_take) * w * 4..]);
        out.extend_from_slice(&last.as_raw()[(h - bottom) * w * 4..]);
        RgbaImage::from_raw(w as u32, (body_take + bottom) as u32, out)
    }

    pub fn result(&self) -> Option<RgbaImage> {
        let first = self.first.as_ref()?;
        let Some((top, bottom)) = self.bands else {
            return Some(first.img.clone());
        };
        let last = &self.prev.as_ref()?.img;
        let (w, h) = (first.img.width() as usize, first.img.height() as usize);
        let mut out = Vec::with_capacity(self.height() * w * 4);
        out.extend_from_slice(&first.img.as_raw()[..top * w * 4]);
        out.extend_from_slice(&self.body);
        out.extend_from_slice(&last.as_raw()[(h - bottom) * w * 4..]);
        RgbaImage::from_raw(w as u32, self.height() as u32, out)
    }
}


/// 测试用：类似文字的伪随机纹理
#[cfg(test)]
fn texture(x: u32, y: u32, seed: u32) -> image::Rgba<u8> {
    let mut v = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    v = (v ^ (v >> 13)).wrapping_mul(1274126177);
    let g = if v % 5 == 0 { (v >> 8) as u8 & 0x7f } else { 250 };
    image::Rgba([g, g, g.saturating_add(3), 255])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成一张每行颜色不同的"长页面"，模拟窗口在上面滚动
    fn page(rows: u32, w: u32) -> RgbaImage {
        RgbaImage::from_fn(w, rows, |x, y| texture(x, y, 1))
    }

    fn view(page: &RgbaImage, off: u32, h: u32, header: u32, footer: u32) -> RgbaImage {
        let w = page.width();
        RgbaImage::from_fn(w, h, |x, y| {
            if y < header {
                image::Rgba([10, 20, 30, 255])
            } else if y >= h - footer {
                image::Rgba([200, 100, 50, 255])
            } else {
                *page.get_pixel(x, off + y)
            }
        })
    }

    #[test]
    fn stitches_with_fixed_bands() {
        let p = page(2000, 120);
        let (h, header, footer) = (300, 40, 30);
        let mut s = Stitcher::new();
        let mut off = 0;
        while off + h <= 1600 {
            s.push(view(&p, off, h, header, footer));
            off += 53;
        }
        let out = s.result().unwrap();
        let last_off = off - 53;
        assert_eq!(out.height(), last_off + h);
        // 预览用的尾部与完整结果末尾一致
        let tail = s.tail(500).unwrap();
        let off_t = out.height() - tail.height();
        for y in [0, tail.height() / 2, tail.height() - 1] {
            assert_eq!(tail.get_pixel(7, y), out.get_pixel(7, off_t + y));
        }
        // 正文与原页面一致
        for y in header..(out.height() - footer) {
            assert_eq!(out.get_pixel(5, y), p.get_pixel(5, y));
        }
    }
}

#[cfg(test)]
mod sidebar {
    use super::*;

    /// 左侧固定侧边栏 + 右侧内容滚动（文档站、带目录的网页常见）
    #[test]
    fn fixed_sidebar() {
        let page = RgbaImage::from_fn(300, 3000, |x, y| texture(x, y, 2));
        let side = RgbaImage::from_fn(80, 400, |x, y| texture(x, y, 3));
        let view = |off: u32| {
            RgbaImage::from_fn(380, 400, |x, y| if x < 80 { *side.get_pixel(x, y) } else { *page.get_pixel(x - 80, off + y) })
        };
        let mut s = Stitcher::new();
        let mut lost = 0;
        for i in 0..20 {
            if let Push::Lost = s.push(view(i * 60)) {
                lost += 1;
            }
        }
        println!("lost = {lost}, height = {}", s.height());
        assert_eq!(lost, 0);
        assert_eq!(s.height(), 19 * 60 + 400);
    }
}

#[cfg(test)]
mod floating {
    use super::*;

    /// 内容区上悬浮一个固定按钮（"回到顶部"之类），同时有吸顶标题栏
    #[test]
    fn floating_button_and_header() {
        let page = RgbaImage::from_fn(400, 4000, |x, y| texture(x, y, 4));
        let view = |off: u32| {
            RgbaImage::from_fn(400, 500, |x, y| {
                if y < 50 {
                    image::Rgba([30, 30, 30, 255])
                } else if (300..360).contains(&x) && (380..440).contains(&y) {
                    image::Rgba([0, 120, 255, 255])
                } else {
                    *page.get_pixel(x, off + y)
                }
            })
        };
        let mut s = Stitcher::new();
        let mut lost = 0;
        for i in 0..30 {
            if let Push::Lost = s.push(view(i * 45)) {
                lost += 1;
            }
        }
        assert_eq!(lost, 0);
        assert_eq!(s.height(), 29 * 45 + 500);
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    #[test]
    #[ignore]
    fn timing() {
        let page = RgbaImage::from_fn(1300, 6000, |x, y| texture(x, y, 5));
        let mut s = Stitcher::new();
        let t = std::time::Instant::now();
        for i in 0..20 {
            s.push(image::imageops::crop_imm(&page, 0, i * 150, 1300, 900).to_image());
        }
        println!("20 帧 1300x900 用时 {:?}，高度 {}", t.elapsed(), s.height());
    }
}
