//! The unknown-word model (a port of dizione's `g2p.py`): letters -> CMUdict
//! phonemes. One forward pass per word: letter embeddings (each letter given
//! `slots` positions), residual convolutions, a bidirectional GRU and a CTC
//! output layer read greedily.

use serde_json::Value;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

struct Array {
    shape: Vec<usize>,
    data: Vec<f32>,
}

impl Array {
    fn rows(&self) -> usize {
        self.shape[0]
    }
    fn cols(&self) -> usize {
        if self.shape.len() > 1 {
            self.shape[1]
        } else {
            1
        }
    }
}

/// The stored (uncompressed) members of a `.npz` file, as float32 arrays.
fn read_npz(bytes: &[u8]) -> Result<HashMap<String, Array>, String> {
    let u16le = |p: usize| u16::from_le_bytes([bytes[p], bytes[p + 1]]) as usize;
    let u32le = |p: usize| {
        u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as usize
    };
    let mut out = HashMap::new();
    let mut p = 0;
    while p + 30 <= bytes.len() && u32le(p) == 0x0403_4b50 {
        let (method, mut size) = (u16le(p + 8), u32le(p + 18));
        let (name_len, extra_len) = (u16le(p + 26), u16le(p + 28));
        let name = String::from_utf8_lossy(&bytes[p + 30..p + 30 + name_len]).to_string();
        let data_at = p + 30 + name_len + extra_len;
        if size == 0xFFFF_FFFF {
            // numpy writes zip64 headers: the sizes (8 bytes each) are in the extra field
            let mut e = p + 30 + name_len;
            while e + 4 <= data_at {
                let (tag, len) = (u16le(e), u16le(e + 2));
                if tag == 1 && len >= 16 {
                    size = u64::from_le_bytes(bytes[e + 12..e + 20].try_into().unwrap()) as usize;
                }
                e += 4 + len;
            }
        }
        if method != 0 {
            return Err("g2p.npz is compressed".into());
        }
        let member = &bytes[data_at..data_at + size];
        out.insert(
            name.trim_end_matches(".npy").to_string(),
            parse_npy(member)?,
        );
        p = data_at + size;
    }
    Ok(out)
}

fn parse_npy(b: &[u8]) -> Result<Array, String> {
    if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
        return Err("not a .npy member".into());
    }
    let (header_len, start) = if b[6] == 1 {
        (u16::from_le_bytes([b[8], b[9]]) as usize, 10)
    } else {
        (u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize, 12)
    };
    let header = String::from_utf8_lossy(&b[start..start + header_len]).to_string();
    if !header.contains("'<f4'") || header.contains("True") {
        return Err(format!("unexpected array layout: {header}"));
    }
    let shape_text = header
        .split("'shape': (")
        .nth(1)
        .and_then(|s| s.split(')').next())
        .ok_or("no shape")?;
    let shape: Vec<usize> = shape_text
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let data = b[start + header_len..]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok(Array { shape, data })
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// `x` (frames x in) times `w` transposed (out x in) plus nothing: (frames x out).
fn matmul_t(x: &[f32], frames: usize, inn: usize, w: &Array) -> Vec<f32> {
    let out = w.rows();
    let mut r = vec![0.0f32; frames * out];
    for t in 0..frames {
        let row = &x[t * inn..(t + 1) * inn];
        for o in 0..out {
            let wr = &w.data[o * inn..(o + 1) * inn];
            r[t * out + o] = row.iter().zip(wr).map(|(a, b)| a * b).sum();
        }
    }
    r
}

fn layer_norm(x: &[f32], frames: usize, dim: usize, weight: &Array, bias: &Array) -> Vec<f32> {
    let mut out = vec![0.0f32; x.len()];
    for t in 0..frames {
        let row = &x[t * dim..(t + 1) * dim];
        let mean = row.iter().sum::<f32>() / dim as f32;
        let var = row.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / dim as f32;
        let inv = 1.0 / (var + 1e-5).sqrt();
        for d in 0..dim {
            out[t * dim + d] = (row[d] - mean) * inv * weight.data[d] + bias.data[d];
        }
    }
    out
}

/// x: (frames, channels); weight: (out, in, kernel), zero padding.
fn conv(x: &[f32], frames: usize, channels: usize, weight: &Array, bias: &Array) -> Vec<f32> {
    let (out_c, in_c, kernel) = (weight.shape[0], weight.shape[1], weight.shape[2]);
    let pad = kernel / 2;
    let mut out = vec![0.0f32; frames * out_c];
    for t in 0..frames {
        for o in 0..out_c {
            let mut acc = bias.data[o];
            for k in 0..kernel {
                let src = t + k;
                if src < pad || src - pad >= frames {
                    continue;
                }
                let row = &x[(src - pad) * channels..(src - pad + 1) * channels];
                let wbase = (o * in_c) * kernel + k;
                for i in 0..in_c {
                    acc += row[i] * weight.data[wbase + i * kernel];
                }
            }
            out[t * out_c + o] = acc;
        }
    }
    out
}

/// `weights` are w_ih, w_hh, b_ih, b_hh.
fn gru(x: &[f32], frames: usize, inn: usize, weights: [&Array; 4], reverse: bool) -> Vec<f32> {
    let [w_ih, w_hh, b_ih, b_hh] = weights;
    let size = w_hh.cols();
    let mut inputs = matmul_t(x, frames, inn, w_ih);
    for t in 0..frames {
        for j in 0..3 * size {
            inputs[t * 3 * size + j] += b_ih.data[j];
        }
    }
    let mut state = vec![0.0f32; size];
    let mut out = vec![0.0f32; frames * size];
    let order: Vec<usize> = if reverse {
        (0..frames).rev().collect()
    } else {
        (0..frames).collect()
    };
    for t in order {
        let mut recurrent = vec![0.0f32; 3 * size];
        for j in 0..3 * size {
            let wr = &w_hh.data[j * size..(j + 1) * size];
            recurrent[j] = wr.iter().zip(&state).map(|(a, b)| a * b).sum::<f32>() + b_hh.data[j];
        }
        let base = t * 3 * size;
        for j in 0..size {
            let reset = sigmoid(inputs[base + j] + recurrent[j]);
            let update = sigmoid(inputs[base + size + j] + recurrent[size + j]);
            let candidate = (inputs[base + 2 * size + j] + reset * recurrent[2 * size + j]).tanh();
            state[j] = (1.0 - update) * candidate + update * state[j];
            out[t * size + j] = state[j];
        }
    }
    out
}

pub struct Model {
    weights: HashMap<String, Array>,
    letters: HashMap<char, usize>,
    symbols: Vec<String>,
    slots: usize,
    convs: usize,
    layers: usize,
}

impl Model {
    pub fn new(settings_json: &str, npz: &[u8]) -> Result<Self, String> {
        let s: Value = serde_json::from_str(settings_json).map_err(|e| e.to_string())?;
        let letters = s["letters"]
            .as_str()
            .ok_or("g2p.json: letters")?
            .chars()
            .enumerate()
            .map(|(i, c)| (c, i + 1))
            .collect();
        let symbols = s["symbols"]
            .as_array()
            .ok_or("g2p.json: symbols")?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let get = |k: &str| {
            s[k].as_u64()
                .map(|v| v as usize)
                .ok_or(format!("g2p.json: {k}"))
        };
        Ok(Self {
            weights: read_npz(npz)?,
            letters,
            symbols,
            slots: get("slots")?,
            convs: get("convs")?,
            layers: get("layers")?,
        })
    }

    /// The word reduced to the letters the model knows (accents removed).
    pub fn spellable(&self, word: &str) -> String {
        let plain: String = word.to_lowercase().nfkd().collect();
        let kept: String = plain
            .chars()
            .filter(|c| self.letters.contains_key(c))
            .collect();
        kept.trim_matches('\'').to_string()
    }

    fn w(&self, name: &str) -> &Array {
        &self.weights[name]
    }

    /// CMUdict symbols with stress digits for `word` (letters and apostrophes).
    pub fn predict(&self, word: &str) -> Vec<String> {
        let word = self.spellable(word);
        if word.is_empty() {
            return Vec::new();
        }
        let ids: Vec<usize> = word.chars().map(|c| self.letters[&c]).collect();
        let embed = self.w("embed__weight");
        let slot = self.w("slot");
        let dim = embed.cols();
        let frames = ids.len() * self.slots;
        let mut x = vec![0.0f32; frames * dim];
        for (n, &id) in ids.iter().enumerate() {
            for s in 0..self.slots {
                for d in 0..dim {
                    x[(n * self.slots + s) * dim + d] =
                        embed.data[id * dim + d] + slot.data[s * dim + d];
                }
            }
        }
        for block in 0..self.convs {
            let normed = layer_norm(
                &x,
                frames,
                dim,
                self.w(&format!("norms__{block}__weight")),
                self.w(&format!("norms__{block}__bias")),
            );
            let c = conv(
                &normed,
                frames,
                dim,
                self.w(&format!("convs__{block}__weight")),
                self.w(&format!("convs__{block}__bias")),
            );
            for (xi, ci) in x.iter_mut().zip(c) {
                *xi += ci.max(0.0);
            }
        }
        let mut width = dim;
        for layer in 0..self.layers {
            let mut both: Vec<Vec<f32>> = Vec::new();
            let mut size = 0;
            for (suffix, reverse) in [("", false), ("_reverse", true)] {
                let g = |k: &str| self.w(&format!("gru__{k}_l{layer}{suffix}"));
                size = g("weight_hh").cols();
                let weights = [g("weight_ih"), g("weight_hh"), g("bias_ih"), g("bias_hh")];
                both.push(gru(&x, frames, width, weights, reverse));
            }
            let mut joined = vec![0.0f32; frames * 2 * size];
            for t in 0..frames {
                joined[t * 2 * size..t * 2 * size + size]
                    .copy_from_slice(&both[0][t * size..(t + 1) * size]);
                joined[t * 2 * size + size..(t + 1) * 2 * size]
                    .copy_from_slice(&both[1][t * size..(t + 1) * size]);
            }
            x = joined;
            width = 2 * size;
        }
        let logits = matmul_t(&x, frames, width, self.w("out__weight"));
        let classes = self.w("out__weight").rows();
        let bias = self.w("out__bias");
        let (mut out, mut previous) = (Vec::new(), 0usize);
        for t in 0..frames {
            let mut label = 0;
            let mut best = f32::NEG_INFINITY;
            for c in 0..classes {
                let v = logits[t * classes + c] + bias.data[c];
                if v > best {
                    best = v;
                    label = c;
                }
            }
            if label != previous && label != 0 {
                out.push(self.symbols[label - 1].clone());
            }
            previous = label;
        }
        out
    }
}
