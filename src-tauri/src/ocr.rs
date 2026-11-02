//! PaddleOCR 离线识别（ONNX Runtime）：文字检测（DB）+ 文字识别（CTC）
use std::path::Path;

use anyhow::{anyhow, Context};
use image::{imageops, Rgb, RgbImage};
use imageproc::contours::{find_contours, BorderType};
use imageproc::geometry::min_area_rect;
use imageproc::point::Point;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct OcrLine {
    pub text: String,
    pub score: f32,
    /// 四个角点（左上、右上、右下、左下），原图像素坐标
    #[serde(rename = "box")]
    pub quad: [[f32; 2]; 4],
}

pub struct Engine {
    _env: ort::environment::Environment,
    det: Session,
    rec: Session,
    /// 下标 0 为 CTC 空白
    chars: Vec<String>,
}

fn e2a<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow!("{e}")
}

fn session(env: &ort::environment::Environment, path: &Path) -> anyhow::Result<Session> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    Session::builder(env)
        .map_err(e2a)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(e2a)?
        .with_intra_threads(threads)
        .map_err(e2a)?
        .commit_from_file(path)
        .map_err(e2a)
        .with_context(|| format!("加载模型失败: {}", path.display()))
}

const DET_MAX_SIDE: u32 = 1600;
const DET_TILE: u32 = 1536;
const DET_OVERLAP: u32 = 96;
const REC_H: u32 = 48;

impl Engine {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let env = crate::onnx::env()?;
        let det = session(&env, &dir.join("det.onnx"))?;
        let rec = session(&env, &dir.join("rec.onnx"))?;
        let dict = std::fs::read_to_string(dir.join("dict.txt")).context("读取字典失败")?;
        let mut chars = vec![String::new()];
        chars.extend(dict.lines().map(|l| l.trim_end_matches('\r').to_string()));
        chars.push(" ".into());
        Ok(Self {
            _env: env,
            det,
            rec,
            chars,
        })
    }

    pub fn run(&mut self, img: &RgbImage) -> anyhow::Result<Vec<OcrLine>> {
        let (w, h) = img.dimensions();
        // 长图分块检测，避免整体缩小后文字太小
        let mut quads: Vec<[[f32; 2]; 4]> = Vec::new();
        if h > DET_TILE + DET_OVERLAP && h > w * 2 {
            let mut y0 = 0u32;
            loop {
                let y1 = (y0 + DET_TILE).min(h);
                let tile = imageops::crop_imm(img, 0, y0, w, y1 - y0).to_image();
                let lo = if y0 == 0 { 0.0 } else { (y0 + DET_OVERLAP / 2) as f32 };
                let hi = if y1 == h { h as f32 } else { (y1 - DET_OVERLAP / 2) as f32 };
                for mut q in self.detect(&tile)? {
                    for p in q.iter_mut() {
                        p[1] += y0 as f32;
                    }
                    let cy = q.iter().map(|p| p[1]).sum::<f32>() / 4.0;
                    if cy >= lo && cy < hi {
                        quads.push(q);
                    }
                }
                if y1 == h {
                    break;
                }
                y0 = y1 - DET_OVERLAP;
            }
        } else {
            quads = self.detect(img)?;
        }
        sort_quads(&mut quads);

        let crops: Vec<RgbImage> = quads.iter().map(|q| crop_quad(img, q)).collect();
        let texts = self.recognize(&crops)?;
        Ok(quads
            .into_iter()
            .zip(texts)
            .filter(|(_, (t, s))| !t.trim().is_empty() && *s > 0.5)
            .map(|(quad, (text, score))| OcrLine { text, score, quad })
            .collect())
    }

    fn detect(&mut self, img: &RgbImage) -> anyhow::Result<Vec<[[f32; 2]; 4]>> {
        let (w, h) = img.dimensions();
        let ratio = (DET_MAX_SIDE as f32 / w.max(h) as f32).min(1.0);
        let round32 = |v: f32| (((v / 32.0).round() as u32) * 32).max(32);
        let (rw, rh) = (round32(w as f32 * ratio), round32(h as f32 * ratio));
        let resized = imageops::resize(img, rw, rh, imageops::FilterType::Triangle);

        // PaddleOCR 按 BGR 通道顺序做归一化
        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];
        let plane = (rw * rh) as usize;
        let mut data = vec![0f32; 3 * plane];
        for (i, p) in resized.pixels().enumerate() {
            let bgr = [p[2], p[1], p[0]];
            for c in 0..3 {
                data[c * plane + i] = (bgr[c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
        let input = Tensor::from_array(([1usize, 3, rh as usize, rw as usize], data)).map_err(e2a)?;
        let outputs = self.det.run(ort::inputs![input]).map_err(e2a)?;
        let (shape, prob) = outputs[0].try_extract_tensor::<f32>().map_err(e2a)?;
        let (ph, pw) = (shape[2] as u32, shape[3] as u32);

        let bitmap = image::GrayImage::from_fn(pw, ph, |x, y| {
            image::Luma([if prob[(y * pw + x) as usize] > 0.3 { 255 } else { 0 }])
        });
        let sx = w as f32 / pw as f32;
        let sy = h as f32 / ph as f32;
        let mut out = Vec::new();
        for c in find_contours::<i32>(&bitmap).into_iter().take(2000) {
            if c.border_type != BorderType::Outer || c.points.len() < 4 {
                continue;
            }
            let rect = min_area_rect(&c.points);
            let q = order_quad(rect.map(|p| [p.x as f32, p.y as f32]));
            let (side_w, side_h) = quad_size(&q);
            if side_w.min(side_h) < 3.0 {
                continue;
            }
            if box_score(prob, pw, ph, &c.points) < 0.6 {
                continue;
            }
            let q = unclip(&q, 1.6);
            let (uw, uh) = quad_size(&q);
            if uw.min(uh) < 5.0 {
                continue;
            }
            out.push(q.map(|p| {
                [
                    (p[0] * sx).clamp(0.0, w as f32 - 1.0),
                    (p[1] * sy).clamp(0.0, h as f32 - 1.0),
                ]
            }));
        }
        Ok(out)
    }

    fn recognize(&mut self, crops: &[RgbImage]) -> anyhow::Result<Vec<(String, f32)>> {
        let mut results = vec![(String::new(), 0f32); crops.len()];
        // 按宽高比排序后分批，减少填充
        let mut order: Vec<usize> = (0..crops.len()).collect();
        let ratio = |i: usize| crops[i].width() as f32 / crops[i].height().max(1) as f32;
        order.sort_by(|&a, &b| ratio(a).partial_cmp(&ratio(b)).unwrap());
        for batch in order.chunks(8) {
            let max_ratio = batch.iter().map(|&i| ratio(i)).fold(320.0 / 48.0, f32::max);
            let bw = ((REC_H as f32 * max_ratio).ceil() as u32).min(3200);
            let n = batch.len();
            let plane = (REC_H * bw) as usize;
            let mut data = vec![0f32; n * 3 * plane];
            for (bi, &i) in batch.iter().enumerate() {
                let c = &crops[i];
                let tw = ((REC_H as f32 * ratio(i)).ceil() as u32).clamp(1, bw);
                let r = imageops::resize(c, tw, REC_H, imageops::FilterType::Triangle);
                let base = bi * 3 * plane;
                for (x, y, p) in r.enumerate_pixels() {
                    let idx = (y * bw + x) as usize;
                    let bgr = [p[2], p[1], p[0]];
                    for ch in 0..3 {
                        data[base + ch * plane + idx] = (bgr[ch] as f32 / 255.0 - 0.5) / 0.5;
                    }
                }
            }
            let input = Tensor::from_array(([n, 3, REC_H as usize, bw as usize], data)).map_err(e2a)?;
            let outputs = self.rec.run(ort::inputs![input]).map_err(e2a)?;
            let (shape, prob) = outputs[0].try_extract_tensor::<f32>().map_err(e2a)?;
            let (t, classes) = (shape[1] as usize, shape[2] as usize);
            for (bi, &i) in batch.iter().enumerate() {
                let mut text = String::new();
                let mut scores = Vec::new();
                let mut last = 0usize;
                for step in 0..t {
                    let row = &prob[(bi * t + step) * classes..(bi * t + step + 1) * classes];
                    let (idx, p) = row
                        .iter()
                        .enumerate()
                        .fold((0, f32::MIN), |a, (k, &v)| if v > a.1 { (k, v) } else { a });
                    if idx != 0 && idx != last {
                        if let Some(ch) = self.chars.get(idx) {
                            text.push_str(ch);
                            scores.push(p);
                        }
                    }
                    last = idx;
                }
                let score = if scores.is_empty() {
                    0.0
                } else {
                    scores.iter().sum::<f32>() / scores.len() as f32
                };
                results[i] = (text, score);
            }
        }
        Ok(results)
    }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn quad_size(q: &[[f32; 2]; 4]) -> (f32, f32) {
    (dist(q[0], q[1]), dist(q[0], q[3]))
}

/// 角点排序为 左上、右上、右下、左下
fn order_quad(pts: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
    let mut p = pts.to_vec();
    p.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    let (l, r) = p.split_at(2);
    let (tl, bl) = if l[0][1] <= l[1][1] { (l[0], l[1]) } else { (l[1], l[0]) };
    let (tr, br) = if r[0][1] <= r[1][1] { (r[0], r[1]) } else { (r[1], r[0]) };
    [tl, tr, br, bl]
}

/// 文字框内概率均值
fn box_score(prob: &[f32], w: u32, h: u32, pts: &[Point<i32>]) -> f32 {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let (x0, y0) = (x0.max(0) as u32, y0.max(0) as u32);
    let (x1, y1) = ((x1.max(0) as u32).min(w - 1), (y1.max(0) as u32).min(h - 1));
    let mut sum = 0f32;
    let mut n = 0u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            sum += prob[(y * w + x) as usize];
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f32
    }
}

/// 按 DB 论文的方式向外扩张文字框：d = 面积 × ratio / 周长
fn unclip(q: &[[f32; 2]; 4], ratio: f32) -> [[f32; 2]; 4] {
    let (w, h) = quad_size(q);
    let d = w * h * ratio / (2.0 * (w + h)).max(1e-3);
    let c = [
        (q[0][0] + q[1][0] + q[2][0] + q[3][0]) / 4.0,
        (q[0][1] + q[1][1] + q[2][1] + q[3][1]) / 4.0,
    ];
    let u = [(q[1][0] - q[0][0]) / w.max(1e-3), (q[1][1] - q[0][1]) / w.max(1e-3)];
    let v = [(q[3][0] - q[0][0]) / h.max(1e-3), (q[3][1] - q[0][1]) / h.max(1e-3)];
    let (hw, hh) = (w / 2.0 + d, h / 2.0 + d);
    let pt = |su: f32, sv: f32| {
        [
            c[0] + u[0] * hw * su + v[0] * hh * sv,
            c[1] + u[1] * hw * su + v[1] * hh * sv,
        ]
    };
    [pt(-1.0, -1.0), pt(1.0, -1.0), pt(1.0, 1.0), pt(-1.0, 1.0)]
}

/// 从上到下、同一行从左到右
fn sort_quads(q: &mut [[[f32; 2]; 4]]) {
    q.sort_by(|a, b| a[0][1].partial_cmp(&b[0][1]).unwrap());
    for i in 1..q.len() {
        let mut j = i;
        while j > 0 && (q[j][0][1] - q[j - 1][0][1]).abs() < 10.0 && q[j][0][0] < q[j - 1][0][0] {
            q.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn bilinear(img: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    let (w, h) = img.dimensions();
    let x = x.clamp(0.0, w as f32 - 1.0);
    let y = y.clamp(0.0, h as f32 - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let mut out = [0u8; 3];
    for c in 0..3 {
        let v = img.get_pixel(x0, y0)[c] as f32 * (1.0 - fx) * (1.0 - fy)
            + img.get_pixel(x1, y0)[c] as f32 * fx * (1.0 - fy)
            + img.get_pixel(x0, y1)[c] as f32 * (1.0 - fx) * fy
            + img.get_pixel(x1, y1)[c] as f32 * fx * fy;
        out[c] = v.round() as u8;
    }
    Rgb(out)
}

/// 把（可能旋转的）文字框拉正裁出
fn crop_quad(img: &RgbImage, q: &[[f32; 2]; 4]) -> RgbImage {
    let cw = dist(q[0], q[1]).max(dist(q[3], q[2])).round().max(1.0) as u32;
    let ch = dist(q[0], q[3]).max(dist(q[1], q[2])).round().max(1.0) as u32;
    let crop = RgbImage::from_fn(cw, ch, |x, y| {
        let u = (x as f32 + 0.5) / cw as f32;
        let v = (y as f32 + 0.5) / ch as f32;
        let top = [q[0][0] + (q[1][0] - q[0][0]) * u, q[0][1] + (q[1][1] - q[0][1]) * u];
        let bot = [q[3][0] + (q[2][0] - q[3][0]) * u, q[3][1] + (q[2][1] - q[3][1]) * u];
        bilinear(img, top[0] + (bot[0] - top[0]) * v, top[1] + (bot[1] - top[1]) * v)
    });
    // 竖排文字旋转为横排
    if ch as f32 >= cw as f32 * 1.5 {
        imageops::rotate270(&crop)
    } else {
        crop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// cargo test --lib ocr -- --ignored --nocapture
    #[test]
    #[ignore]
    fn recognizes_sample() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut engine = Engine::load(&root.join("resources/ocr")).unwrap();
        let img = image::open(root.join("tests_ocr_sample.png")).unwrap().to_rgb8();
        let t = std::time::Instant::now();
        let lines = engine.run(&img).unwrap();
        println!("耗时 {:?}", t.elapsed());
        for l in &lines {
            println!("{:.3} {:?} {}", l.score, l.quad[0], l.text);
        }
        let all: String = lines.iter().map(|l| l.text.as_str()).collect();
        assert!(all.contains("文字识别"));
        assert!(all.contains("translation"));
    }
}
