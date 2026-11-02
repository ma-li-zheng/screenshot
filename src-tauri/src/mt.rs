//! 内置离线翻译：Opus-MT（Helsinki-NLP，CC-BY-4.0）中↔英，INT8 量化 ONNX。
//! 分词为 SentencePiece Unigram（Viterbi），解码为带 KV 缓存的贪心搜索。
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context};
use ort::environment::Environment;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::{Session, SessionInputValue};
use ort::value::Tensor;
use unicode_normalization::UnicodeNormalization;

const EOS: i64 = 0;
const UNK: i64 = 1;
const PAD: i64 = 65000;
const LAYERS: usize = 6;
const HEADS: usize = 8;
const HEAD_DIM: usize = 64;
const MAX_SRC: usize = 256;
const BATCH: usize = 8;

fn e2a<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow!("{e}")
}

pub struct Translator {
    _env: Environment,
    enc: Session,
    dec: Session,
    /// id → 词片
    vocab: Vec<String>,
    /// 词片 → (id, 分数)
    pieces: HashMap<String, (i64, f64)>,
    max_piece_chars: usize,
}

/// 一个张量的形状和数据
struct Owned {
    shape: Vec<usize>,
    data: Vec<f32>,
}

impl Translator {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let env = crate::onnx::env()?;
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
        let session = |name: &str| -> anyhow::Result<Session> {
            Session::builder(&env)
                .map_err(e2a)?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(e2a)?
                .with_intra_threads(threads)
                .map_err(e2a)?
                .commit_from_file(dir.join(name))
                .map_err(e2a)
                .with_context(|| format!("加载翻译模型失败: {name}"))
        };
        let enc = session("encoder.onnx")?;
        let dec = session("decoder.onnx")?;

        let tok: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("tokenizer.json")).context("读取分词表失败")?)?;
        let list = tok["model"]["vocab"].as_array().ok_or_else(|| anyhow!("分词表格式不正确"))?;
        let mut vocab = Vec::with_capacity(list.len());
        let mut pieces = HashMap::with_capacity(list.len());
        let mut max_piece_chars = 1;
        for (id, item) in list.iter().enumerate() {
            let piece = item[0].as_str().unwrap_or_default().to_string();
            let score = item[1].as_f64().unwrap_or(-20.0);
            let id = id as i64;
            if id != EOS && id != UNK && id != PAD && !piece.is_empty() {
                max_piece_chars = max_piece_chars.max(piece.chars().count());
                pieces.insert(piece.clone(), (id, score));
            }
            vocab.push(piece);
        }
        Ok(Self {
            _env: env,
            enc,
            dec,
            vocab,
            pieces,
            max_piece_chars,
        })
    }

    /// SentencePiece 风格分词：NFKC 规范化，按空白切词，每个词前加 ▁，Viterbi 求最优切分
    fn encode(&self, text: &str) -> Vec<i64> {
        let norm: String = text.nfkc().collect();
        let mut ids = Vec::new();
        for word in norm.split_whitespace() {
            let chars: Vec<char> = std::iter::once('\u{2581}').chain(word.chars()).collect();
            let n = chars.len();
            let mut best = vec![f64::NEG_INFINITY; n + 1];
            let mut back: Vec<(usize, i64)> = vec![(0, UNK); n + 1];
            best[0] = 0.0;
            for i in 0..n {
                if best[i] == f64::NEG_INFINITY {
                    continue;
                }
                let mut s = String::new();
                for len in 1..=self.max_piece_chars.min(n - i) {
                    s.push(chars[i + len - 1]);
                    if let Some(&(id, score)) = self.pieces.get(&s) {
                        let cand = best[i] + score;
                        if cand > best[i + len] {
                            best[i + len] = cand;
                            back[i + len] = (i, id);
                        }
                    }
                }
                // 词表里没有的字符记为 <unk>
                let cand = best[i] - 30.0;
                if cand > best[i + 1] {
                    best[i + 1] = cand;
                    back[i + 1] = (i, UNK);
                }
            }
            let mut seg = Vec::new();
            let mut j = n;
            while j > 0 {
                let (i, id) = back[j];
                seg.push(id);
                j = i;
            }
            seg.reverse();
            for id in seg {
                if !(id == UNK && ids.last() == Some(&UNK)) {
                    ids.push(id);
                }
            }
        }
        ids.truncate(MAX_SRC - 1);
        ids.push(EOS);
        ids
    }

    fn decode(&self, ids: &[i64]) -> String {
        let mut s = String::new();
        for &id in ids {
            if id == EOS || id == PAD || id == UNK {
                continue;
            }
            if let Some(p) = self.vocab.get(id as usize) {
                s.push_str(p);
            }
        }
        tidy_cjk(s.replace('\u{2581}', " ").trim())
    }

    /// 逐行翻译，返回与输入一一对应的结果
    pub fn translate(&mut self, texts: &[String]) -> anyhow::Result<Vec<String>> {
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(BATCH) {
            out.extend(self.translate_batch(chunk)?);
        }
        Ok(out)
    }

    fn translate_batch(&mut self, texts: &[String]) -> anyhow::Result<Vec<String>> {
        let b = texts.len();
        let encoded: Vec<Vec<i64>> = texts.iter().map(|t| self.encode(t)).collect();
        let s = encoded.iter().map(|e| e.len()).max().unwrap_or(1);
        let mut ids = vec![PAD; b * s];
        let mut mask = vec![0i64; b * s];
        for (i, e) in encoded.iter().enumerate() {
            ids[i * s..i * s + e.len()].copy_from_slice(e);
            mask[i * s..i * s + e.len()].iter_mut().for_each(|m| *m = 1);
        }

        // 编码器
        let hidden = {
            let outputs = self
                .enc
                .run(ort::inputs![
                    "input_ids" => Tensor::from_array(([b, s], ids)).map_err(e2a)?,
                    "attention_mask" => Tensor::from_array(([b, s], mask.clone())).map_err(e2a)?,
                ])
                .map_err(e2a)?;
            let (shape, data) = outputs["last_hidden_state"].try_extract_tensor::<f32>().map_err(e2a)?;
            Owned {
                shape: shape.iter().map(|&d| d as usize).collect(),
                data: data.to_vec(),
            }
        };

        // 解码器：贪心搜索 + KV 缓存
        let empty = |len: usize| Owned {
            shape: vec![b, HEADS, len, HEAD_DIM],
            data: Vec::new(),
        };
        let mut past: Vec<Owned> = (0..LAYERS * 4).map(|_| empty(0)).collect();
        let mut tokens = vec![PAD; b];
        let mut result: Vec<Vec<i64>> = vec![Vec::new(); b];
        let mut done = vec![false; b];
        let max_steps = (s * 3 + 10).min(MAX_SRC);
        let kinds = ["decoder.key", "decoder.value", "encoder.key", "encoder.value"];

        for step in 0..max_steps {
            let mut inputs: Vec<(Cow<'static, str>, SessionInputValue<'_>)> = vec![
                ("input_ids".into(), Tensor::from_array(([b, 1], tokens.clone())).map_err(e2a)?.into()),
                ("encoder_attention_mask".into(), Tensor::from_array(([b, s], mask.clone())).map_err(e2a)?.into()),
                (
                    "encoder_hidden_states".into(),
                    Tensor::from_array((hidden.shape.clone(), hidden.data.clone())).map_err(e2a)?.into(),
                ),
                ("use_cache_branch".into(), Tensor::from_array(([1usize], vec![step > 0])).map_err(e2a)?.into()),
            ];
            for layer in 0..LAYERS {
                for (k, kind) in kinds.iter().enumerate() {
                    let p = &past[layer * 4 + k];
                    inputs.push((
                        format!("past_key_values.{layer}.{kind}").into(),
                        Tensor::from_array((p.shape.clone(), p.data.clone())).map_err(e2a)?.into(),
                    ));
                }
            }
            let outputs = self.dec.run(inputs).map_err(e2a)?;
            let (lshape, logits) = outputs["logits"].try_extract_tensor::<f32>().map_err(e2a)?;
            let vocab = lshape[2] as usize;
            for i in 0..b {
                if done[i] {
                    tokens[i] = PAD;
                    continue;
                }
                let row = &logits[i * vocab..(i + 1) * vocab];
                let mut best = (0usize, f32::MIN);
                for (id, &v) in row.iter().enumerate() {
                    if id as i64 != PAD && v > best.1 {
                        best = (id, v);
                    }
                }
                let id = best.0 as i64;
                tokens[i] = id;
                if id == EOS {
                    done[i] = true;
                } else {
                    result[i].push(id);
                }
            }
            if done.iter().all(|&d| d) {
                break;
            }
            // 更新 KV 缓存；编码器部分只需第一步的结果
            for layer in 0..LAYERS {
                for (k, kind) in kinds.iter().enumerate() {
                    if step > 0 && k >= 2 {
                        continue;
                    }
                    let (shape, data) = outputs[format!("present.{layer}.{kind}").as_str()]
                        .try_extract_tensor::<f32>()
                        .map_err(e2a)?;
                    past[layer * 4 + k] = Owned {
                        shape: shape.iter().map(|&d| d as usize).collect(),
                        data: data.to_vec(),
                    };
                }
            }
        }
        Ok(result.iter().map(|r| self.decode(r)).collect())
    }
}

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
}

fn is_cjk_punct(c: char) -> bool {
    "。，、；：！？“”‘’（）《》【】…—".contains(c)
}

/// 去掉中文译文里多余的空格：中文标点前后、两个汉字之间
fn tidy_cjk(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let prev = out.chars().last();
            let next = chars[i + 1..].iter().copied().find(|&n| n != ' ');
            let drop = prev.is_none_or(|p| p == ' ' || is_cjk_punct(p))
                || next.is_none_or(is_cjk_punct)
                || matches!((prev, next), (Some(p), Some(n)) if is_han(p) && is_han(n));
            if drop {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidies_chinese_spacing() {
        assert_eq!(tidy_cjk("屏幕截图工具支持 OCR 和翻译 。"), "屏幕截图工具支持 OCR 和翻译。");
        assert_eq!(tidy_cjk("点击 保存 ， 然后 关闭"), "点击保存，然后关闭");
        assert_eq!(tidy_cjk("Hello, world !"), "Hello, world !");
    }

    /// 需要先下载模型：cargo test --lib mt -- --ignored --nocapture
    #[test]
    #[ignore]
    fn translate_zh_en() {
        let dir = std::env::var("OPUS_ZH_EN").unwrap_or_else(|_| r"E:\screenshot\.downloads\opus\zh-en".into());
        let mut t = Translator::load(Path::new(&dir)).unwrap();
        let src: Vec<String> = [
            "截图工具支持文字识别和翻译功能。",
            "今天天气很好，我们去公园散步吧。",
            "订单编号：20261102-8848，金额 ¥1,299.00",
            "请在设置中下载离线翻译模型",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        println!("{:?}", t.encode(&src[0]));
        let start = std::time::Instant::now();
        let out = t.translate(&src).unwrap();
        println!("耗时 {:?}", start.elapsed());
        for (a, b) in src.iter().zip(&out) {
            println!("{a}\n  → {b}");
        }
        assert!(out[0].to_lowercase().contains("screenshot"));
        assert!(out[1].to_lowercase().contains("park"));
    }
}
