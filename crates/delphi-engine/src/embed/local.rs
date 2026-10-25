//! The local embedding model: a BERT-family encoder run on the CPU with `candle` (`specs/004` §3.5).
//!
//! A model ships as one file (`DLPHMDL1`): the tokenizer, the model's config and its weights as int8 with one
//! scale per row, which is a quarter of the f32 size and indistinguishable from it in retrieval (`delphi kb eval`
//! shows the numbers). The file is either compiled into the binary (`embedded-model`) or found on disk.
//! Weights are widened to f32 when the model is loaded; the matrix products run on Apple's Accelerate where the
//! platform has it.

#[cfg(target_os = "macos")]
extern crate accelerate_src;

use super::{normalize, Embedder};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

mod rerank;
pub use rerank::{open as open_reranker, open_bytes as open_reranker_bytes, CandleReranker};

const MAGIC: &[u8; 8] = b"DLPHMDL1";
/// Tensors with fewer elements than this stay f32 (biases, norms, small tables).
const QUANTIZE_FROM: usize = 4096;
/// Most tokens in one forward pass; longer inputs are batched less deeply.
const BATCH_TOKENS: usize = 4096;
const MAX_BATCH: usize = 16;

/// What the model is and how to talk to it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    /// Recorded in the index; a different id means every vector is made again.
    pub id: String,
    /// `bert` or `modernbert`.
    pub kind: String,
    /// `cls` or `mean`.
    pub pooling: String,
    pub query_prefix: String,
    pub doc_prefix: String,
    pub max_tokens: usize,
    pub min_similarity: f32,
    pub dim: usize,
    /// Where the weights came from (repository and revision), for provenance.
    pub source: String,
}

#[derive(Serialize, Deserialize)]
struct TensorEntry {
    name: String,
    shape: Vec<usize>,
    /// `f32` or `q8` (int8 values, then one f32 scale per row).
    enc: String,
    off: u64,
    len: u64,
}

#[derive(Serialize, Deserialize)]
struct Header {
    meta: Meta,
    files: BTreeMap<String, (u64, u64)>,
    tensors: Vec<TensorEntry>,
}

#[derive(Debug, Default)]
pub struct PackReport {
    pub tensors: usize,
    pub quantized: usize,
    pub bytes: usize,
    pub max_abs_error: f32,
}

fn to_f32(t: &Tensor) -> Result<Vec<f32>, String> {
    t.to_dtype(DType::F32).and_then(|t| t.flatten_all()).and_then(|t| t.to_vec1::<f32>()).map_err(|e| e.to_string())
}

/// Read a Hugging Face model directory (`config.json`, `tokenizer.json`, `model.safetensors`) and write the
/// single-file package.
pub fn pack(dir: &Path, meta: &Meta, out: &Path) -> Result<PackReport, String> {
    let read = |n: &str| std::fs::read(dir.join(n)).map_err(|e| format!("{}: {}", dir.join(n).display(), e));
    let weights = candle_core::safetensors::load(dir.join("model.safetensors"), &Device::Cpu).map_err(|e| e.to_string())?;
    let mut blob: Vec<u8> = Vec::new();
    let mut files = BTreeMap::new();
    for n in ["config.json", "tokenizer.json"] {
        let b = read(n)?;
        files.insert(n.to_string(), (blob.len() as u64, b.len() as u64));
        blob.extend(b);
    }
    let mut names: Vec<&String> = weights.keys().collect();
    names.sort();
    let (mut tensors, mut rep) = (Vec::new(), PackReport::default());
    for name in names {
        let t = &weights[name];
        if !matches!(t.dtype(), DType::F32 | DType::F16 | DType::BF16) {
            continue; // position_ids and the like
        }
        let shape = t.dims().to_vec();
        let data = to_f32(t)?;
        let off = blob.len() as u64;
        let enc = if shape.len() == 2 && data.len() >= QUANTIZE_FROM {
            let (rows, cols) = (shape[0], shape[1]);
            let mut scales = Vec::with_capacity(rows);
            for r in 0..rows {
                let row = &data[r * cols..(r + 1) * cols];
                let scale = (row.iter().fold(0.0f32, |m, x| m.max(x.abs())) / 127.0).max(f32::MIN_POSITIVE);
                for x in row {
                    let q = (x / scale).round().clamp(-127.0, 127.0);
                    rep.max_abs_error = rep.max_abs_error.max((q * scale - x).abs());
                    blob.push(q as i8 as u8);
                }
                scales.push(scale);
            }
            scales.iter().for_each(|s| blob.extend(s.to_le_bytes()));
            rep.quantized += 1;
            "q8"
        } else {
            data.iter().for_each(|x| blob.extend(x.to_le_bytes()));
            "f32"
        };
        rep.tensors += 1;
        // The encoder-only ModernBERT checkpoints omit the `model.` prefix its loader expects.
        let name = if meta.kind == "modernbert" && !name.starts_with("model.") { format!("model.{}", name) } else { name.clone() };
        tensors.push(TensorEntry { name, shape, enc: enc.into(), off, len: blob.len() as u64 - off });
    }
    let header = serde_json::to_vec(&Header { meta: meta.clone(), files, tensors }).map_err(|e| e.to_string())?;
    let mut file = Vec::with_capacity(blob.len() + header.len() + 16);
    file.extend(MAGIC);
    file.extend((header.len() as u64).to_le_bytes());
    file.extend(header);
    file.extend(blob);
    rep.bytes = file.len();
    std::fs::write(out, file).map_err(|e| format!("{}: {}", out.display(), e))?;
    Ok(rep)
}

enum Net {
    Bert(Box<candle_transformers::models::bert::BertModel>),
    Modern(candle_transformers::models::modernbert::ModernBert),
}

pub struct CandleEmbedder {
    meta: Meta,
    net: Net,
    tok: Tokenizer,
    threads: usize,
    dev: Device,
    dtype: DType,
}

/// Where the models run, for `delphi kb model info`.
pub fn device_name() -> &'static str {
    if pick_device().0.is_cpu() {
        "the CPU"
    } else {
        "the GPU (Metal)"
    }
}

/// The GPU on a Mac that has one, otherwise the CPU; `DELPHI_EMBED_DEVICE=cpu` forces the CPU. Always f32: on
/// Metal, f16 was measured to give wrong vectors (recall 0) and to be slower, so it is not offered.
fn pick_device() -> (Device, DType) {
    if std::env::var("DELPHI_EMBED_DEVICE").is_ok_and(|v| v == "cpu") {
        return (Device::Cpu, DType::F32);
    }
    match Device::new_metal(0) {
        Ok(gpu) => (gpu, DType::F32),
        Err(_) => (Device::Cpu, DType::F32),
    }
}

/// The package's metadata without loading the weights.
pub fn peek(bytes: &[u8]) -> Result<Meta, String> {
    Ok(split(bytes)?.0.meta)
}

/// `peek` for a package on disk: only the header is read, not the weights.
pub fn peek_file(path: &Path) -> Result<Meta, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut head = [0u8; 16];
    f.read_exact(&mut head).map_err(|e| e.to_string())?;
    let n = u64::from_le_bytes(head[8..16].try_into().unwrap()) as usize;
    if n > 64 << 20 {
        return Err("not a Delphi model package".into());
    }
    let mut all = head.to_vec();
    all.resize(16 + n, 0);
    f.read_exact(&mut all[16..]).map_err(|e| e.to_string())?;
    peek(&all)
}

fn split(bytes: &[u8]) -> Result<(Header, &[u8]), String> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
        return Err("not a Delphi model package".into());
    }
    let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let header: Header = serde_json::from_slice(bytes.get(16..16 + n).ok_or("truncated model package")?).map_err(|e| e.to_string())?;
    Ok((header, &bytes[16 + n..]))
}

/// The package's weights as f32 tensors on `dev` (int8 matrices are widened with their row scales).
fn tensors(header: &Header, blob: &[u8], dev: &Device) -> Result<HashMap<String, Tensor>, String> {
    let slice = |off: u64, len: u64| blob.get(off as usize..(off + len) as usize).ok_or_else(|| "truncated model package".to_string());
    let mut out: HashMap<String, Tensor> = HashMap::new();
    for e in &header.tensors {
        let raw = slice(e.off, e.len)?;
        let n: usize = e.shape.iter().product();
        let data: Vec<f32> = match e.enc.as_str() {
            "f32" => raw.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect(),
            "q8" => {
                let (vals, scales) = raw.split_at(n);
                let cols = e.shape[1];
                let scales: Vec<f32> = scales.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
                vals.iter().enumerate().map(|(i, q)| (*q as i8) as f32 * scales[i / cols]).collect()
            }
            other => return Err(format!("unknown tensor encoding {}", other)),
        };
        out.insert(e.name.clone(), Tensor::from_vec(data, e.shape.clone(), dev).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

impl CandleEmbedder {
    pub fn load(bytes: &[u8]) -> Result<CandleEmbedder, String> {
        let (header, blob) = split(bytes)?;
        let slice = |off: u64, len: u64| blob.get(off as usize..(off + len) as usize).ok_or_else(|| "truncated model package".to_string());
        let file = |n: &str| -> Result<&[u8], String> {
            let (o, l) = header.files.get(n).ok_or_else(|| format!("model package has no {}", n))?;
            slice(*o, *l)
        };
        let (dev, dtype) = pick_device();
        let vb = VarBuilder::from_tensors(tensors(&header, blob, &dev)?, dtype, &dev);
        let cfg = std::str::from_utf8(file("config.json")?).map_err(|e| e.to_string())?;
        let net = match header.meta.kind.as_str() {
            "bert" => {
                let cfg: candle_transformers::models::bert::Config = serde_json::from_str(cfg).map_err(|e| e.to_string())?;
                Net::Bert(Box::new(candle_transformers::models::bert::BertModel::load(vb, &cfg).map_err(|e| e.to_string())?))
            }
            "modernbert" => {
                let cfg: candle_transformers::models::modernbert::Config = serde_json::from_str(cfg).map_err(|e| e.to_string())?;
                Net::Modern(candle_transformers::models::modernbert::ModernBert::load(vb, &cfg).map_err(|e| e.to_string())?)
            }
            other => return Err(format!("unknown model kind {}", other)),
        };
        let mut tok = Tokenizer::from_bytes(file("tokenizer.json")?).map_err(|e| e.to_string())?;
        tok.with_padding(Some(PaddingParams::default()));
        tok.with_truncation(Some(TruncationParams { max_length: header.meta.max_tokens, ..Default::default() })).map_err(|e| e.to_string())?;
        // The GPU takes one batch at a time; on the CPU a few batches in flight keep the cores busy.
        let threads = std::env::var("DELPHI_EMBED_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(if dev.is_cpu() { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4) } else { 1 });
        Ok(CandleEmbedder { meta: header.meta, net, tok, threads, dev, dtype })
    }

    fn forward(&self, batch: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let dev = &self.dev;
        let enc = self.tok.encode_batch(batch.to_vec(), true).map_err(|e| e.to_string())?;
        let (b, l) = (enc.len(), enc[0].get_ids().len());
        let ids: Vec<u32> = enc.iter().flat_map(|e| e.get_ids().to_vec()).collect();
        let mask: Vec<u32> = enc.iter().flat_map(|e| e.get_attention_mask().to_vec()).collect();
        let run = || -> candle_core::Result<Vec<Vec<f32>>> {
            let ids = Tensor::from_vec(ids, (b, l), dev)?;
            let mask = Tensor::from_vec(mask, (b, l), dev)?;
            let hidden = match &self.net {
                Net::Bert(m) => m.forward(&ids, &ids.zeros_like()?, Some(&mask))?,
                Net::Modern(m) => m.forward(&ids, &mask)?,
            };
            let pooled = if self.meta.pooling == "mean" {
                let m = mask.to_dtype(self.dtype)?.unsqueeze(2)?;
                hidden.broadcast_mul(&m)?.sum(1)?.broadcast_div(&m.sum(1)?)?
            } else {
                hidden.narrow(1, 0, 1)?.squeeze(1)?
            };
            pooled.to_dtype(DType::F32)?.to_vec2::<f32>()
        };
        let mut out = run().map_err(|e| e.to_string())?;
        out.iter_mut().for_each(|v| normalize(v));
        Ok(out)
    }

    /// Embed with `prefix`, longest texts first in batches that keep each pass to a bounded number of tokens, on
    /// a few threads. Results are returned in the order of `texts`.
    fn embed_all(&self, texts: &[String], prefix: &str) -> Result<Vec<Vec<f32>>, String> {
        let texts: Vec<String> = texts.iter().map(|t| format!("{}{}", prefix, t)).collect();
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|i| std::cmp::Reverse(texts[*i].len()));
        let mut batches: Vec<Vec<usize>> = Vec::new();
        let mut cur: Vec<usize> = Vec::new();
        for i in order {
            // About four characters to a token, capped by the model's window.
            let tokens = (texts[i].len() / 4 + 2).min(self.meta.max_tokens);
            let longest = cur.first().map(|f| (texts[*f].len() / 4 + 2).min(self.meta.max_tokens)).unwrap_or(tokens);
            if !cur.is_empty() && (cur.len() >= MAX_BATCH || (cur.len() + 1) * longest > BATCH_TOKENS) {
                batches.push(std::mem::take(&mut cur));
            }
            cur.push(i);
        }
        if !cur.is_empty() {
            batches.push(cur);
        }
        let next = AtomicUsize::new(0);
        let results: Mutex<Vec<Option<Vec<f32>>>> = Mutex::new(vec![None; texts.len()]);
        let failure: Mutex<Option<String>> = Mutex::new(None);
        std::thread::scope(|s| {
            for _ in 0..self.threads.min(batches.len()).max(1) {
                s.spawn(|| loop {
                    let n = next.fetch_add(1, Ordering::SeqCst);
                    let Some(ids) = batches.get(n) else { break };
                    let slice: Vec<String> = ids.iter().map(|i| texts[*i].clone()).collect();
                    match self.forward(&slice) {
                        Ok(vs) => {
                            let mut r = results.lock().unwrap();
                            for (i, v) in ids.iter().zip(vs) {
                                r[*i] = Some(v);
                            }
                        }
                        Err(e) => {
                            *failure.lock().unwrap() = Some(e);
                            break;
                        }
                    }
                });
            }
        });
        if let Some(e) = failure.into_inner().unwrap() {
            return Err(e);
        }
        results.into_inner().unwrap().into_iter().map(|v| v.ok_or_else(|| "embedding batch missing".to_string())).collect()
    }
}

impl Embedder for CandleEmbedder {
    fn model_id(&self) -> &str {
        &self.meta.id
    }
    fn dim(&self) -> usize {
        self.meta.dim
    }
    fn min_similarity(&self) -> f32 {
        self.meta.min_similarity
    }
    fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.embed_all(texts, &self.meta.doc_prefix)
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed_all(&[text.to_string()], &self.meta.query_prefix).map(|mut v| v.remove(0))
    }
}

/// Open a package from disk.
pub fn open(path: &Path) -> Result<Arc<dyn Embedder>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    Ok(Arc::new(CandleEmbedder::load(&bytes)?))
}

pub fn open_bytes(bytes: &[u8]) -> Result<Arc<dyn Embedder>, String> {
    Ok(Arc::new(CandleEmbedder::load(bytes)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_nn::VarMap;
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::pre_tokenizers::whitespace::Whitespace;
    use tokenizers::processors::template::TemplateProcessing;

    const CONFIG: &str = r#"{"vocab_size":32,"hidden_size":64,"num_hidden_layers":2,"num_attention_heads":4,"intermediate_size":128,"hidden_act":"gelu","hidden_dropout_prob":0.0,"max_position_embeddings":64,"type_vocab_size":2,"initializer_range":0.02,"layer_norm_eps":1e-12,"pad_token_id":0,"model_type":"bert"}"#;

    /// A small BERT with random weights and a word-level tokenizer, written as a model directory.
    fn tiny_model(dir: &Path) -> candle_transformers::models::bert::BertModel {
        let words = ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "retry", "ladder", "escalates", "zebra", "stripes", "pipe", "buffer", "deadlock"];
        let model = WordLevel::builder().vocab(words.iter().enumerate().map(|(i, w)| (w.to_string(), i as u32)).collect()).unk_token("[UNK]".into()).build().unwrap();
        let mut tok = Tokenizer::new(model);
        tok.with_pre_tokenizer(Some(Whitespace {}));
        tok.with_post_processor(Some(TemplateProcessing::builder().try_single("[CLS] $A [SEP]").unwrap().special_tokens(vec![("[CLS]", 2), ("[SEP]", 3)]).build().unwrap()));
        tok.save(dir.join("tokenizer.json"), false).unwrap();
        std::fs::write(dir.join("config.json"), CONFIG).unwrap();
        let vars = VarMap::new();
        let vb = VarBuilder::from_varmap(&vars, DType::F32, &Device::Cpu);
        let cfg: candle_transformers::models::bert::Config = serde_json::from_str(CONFIG).unwrap();
        let bert = candle_transformers::models::bert::BertModel::load(vb, &cfg).unwrap();
        vars.save(dir.join("model.safetensors")).unwrap();
        bert
    }

    fn meta() -> Meta {
        Meta { id: "tiny-q8".into(), kind: "bert".into(), pooling: "cls".into(), query_prefix: String::new(), doc_prefix: String::new(), max_tokens: 16, min_similarity: 0.0, dim: 64, source: "test".into() }
    }

    #[test]
    fn a_packed_model_loads_and_embeds_like_the_original_weights_to_within_quantization_error() {
        let d = tempfile::tempdir().unwrap();
        let reference = tiny_model(d.path());
        let out = d.path().join("m.dlphmdl");
        let rep = pack(d.path(), &meta(), &out).unwrap();
        assert!(rep.quantized > 0 && rep.max_abs_error < 0.05, "{:?}", rep);
        assert_eq!(peek_file(&out).unwrap(), meta());
        let packed = CandleEmbedder::load(&std::fs::read(&out).unwrap()).unwrap();

        let texts = vec!["retry ladder escalates".to_string(), "zebra stripes".to_string(), "pipe buffer deadlock retry".to_string()];
        let vs = packed.embed_documents(&texts).unwrap();
        assert_eq!(vs.len(), 3);
        for v in &vs {
            assert_eq!(v.len(), 64);
            assert!((v.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-4, "unit length");
        }
        // The same text in a different batch position gives the same vector.
        let alone = packed.embed_documents(&texts[1..2]).unwrap();
        let cos = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        assert!(cos(&alone[0], &vs[1]) > 0.999);
        // And it agrees with the unquantized model run directly.
        let tok = Tokenizer::from_file(d.path().join("tokenizer.json")).unwrap();
        let enc = tok.encode("zebra stripes", true).unwrap();
        let ids = Tensor::from_vec(enc.get_ids().to_vec(), (1, enc.get_ids().len()), &Device::Cpu).unwrap();
        let h = reference.forward(&ids, &ids.zeros_like().unwrap(), None).unwrap();
        let mut want = h.narrow(1, 0, 1).unwrap().squeeze(1).unwrap().to_vec2::<f32>().unwrap().remove(0);
        normalize(&mut want);
        assert!(cos(&want, &vs[1]) > 0.99, "int8 weights keep the direction of the vector: {}", cos(&want, &vs[1]));
        // Different texts are not the same vector.
        assert!(cos(&vs[0], &vs[1]) < 0.999);
        assert_eq!(packed.embed_query("retry").unwrap().len(), 64);
    }

    #[test]
    fn a_damaged_package_is_refused_not_trusted() {
        assert!(CandleEmbedder::load(b"nope").is_err());
        let d = tempfile::tempdir().unwrap();
        tiny_model(d.path());
        let out = d.path().join("m.dlphmdl");
        pack(d.path(), &meta(), &out).unwrap();
        let mut bytes = std::fs::read(&out).unwrap();
        bytes.truncate(bytes.len() / 2);
        assert!(CandleEmbedder::load(&bytes).is_err(), "a truncated package fails to load");
    }
}
