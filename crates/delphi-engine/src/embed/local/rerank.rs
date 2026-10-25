//! The reranker: a cross-encoder that reads a question and a candidate passage together and scores how well the
//! passage answers it (`specs/004` §3.7). It is slower than the embedder because nothing can be precomputed, so it
//! only reorders the few best results of the hybrid search.

use super::{split, tensors, Header};
use crate::embed::Reranker;
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, Linear, Module, VarBuilder};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokenizers::{EncodeInput, PaddingParams, Tokenizer, TruncationParams, TruncationStrategy};

/// Most pairs in one forward pass on the GPU, and on the CPU, where several passes run side by side: candle's
/// element-wise kernels (GELU, layer norm) run on one core, so small batches on several cores are faster than one big one.
const GPU_BATCH: usize = 8;
const CPU_BATCH: usize = 2;

/// The reranker runs on the CPU even on a Mac with a GPU: a cross-encoder this small is many short kernels, and
/// Metal's per-kernel cost made a search about four times slower (about 2.2 s against 0.5 s for ten passages), while
/// the embedder, which reads a query once, is faster on the GPU. `DELPHI_RERANK_DEVICE=metal` overrides.
fn rerank_device() -> (Device, DType) {
    if std::env::var("DELPHI_RERANK_DEVICE").is_ok_and(|v| v == "metal") {
        if let Ok(gpu) = Device::new_metal(0) {
            return (gpu, DType::F32);
        }
    }
    (Device::Cpu, DType::F32)
}

enum Cross {
    /// `BertForSequenceClassification`: the model, the pooler over the first token, then the classifier.
    Bert { model: Box<candle_transformers::models::bert::BertModel>, pooler: Linear, classifier: Linear },
    /// `XLMRobertaForSequenceClassification` (the `bge-reranker` family).
    Xlmr(Box<candle_transformers::models::xlm_roberta::XLMRobertaForSequenceClassification>),
    /// `DebertaV2ForSequenceClassification` (the `mxbai-rerank` family).
    Deberta(Box<candle_transformers::models::debertav2::DebertaV2SeqClassificationModel>),
}

pub struct CandleReranker {
    id: String,
    min_score: Option<f32>,
    net: Cross,
    tok: Tokenizer,
    dev: Device,
    threads: usize,
}

impl CandleReranker {
    pub fn load(bytes: &[u8]) -> Result<CandleReranker, String> {
        let (header, blob): (Header, &[u8]) = split(bytes)?;
        let (dev, dtype) = rerank_device();
        let file = |n: &str| -> Result<&[u8], String> {
            let (o, l) = header.files.get(n).ok_or_else(|| format!("model package has no {}", n))?;
            blob.get(*o as usize..(*o + *l) as usize).ok_or_else(|| "truncated model package".to_string())
        };
        let mut weights = tensors(&header, blob, &dev)?;
        if header.meta.kind == "deberta-cross" {
            weights = weights.into_iter().map(|(k, v)| (k.strip_prefix("deberta.").map(String::from).unwrap_or(k), v)).collect();
        }
        let vb = VarBuilder::from_tensors(weights, dtype, &dev);
        let cfg = std::str::from_utf8(file("config.json")?).map_err(|e| e.to_string())?;
        let err = |e: candle_core::Error| e.to_string();
        let net = match header.meta.kind.as_str() {
            "bert-cross" => {
                let cfg: candle_transformers::models::bert::Config = serde_json::from_str(cfg).map_err(|e| e.to_string())?;
                let model = candle_transformers::models::bert::BertModel::load(vb.clone(), &cfg).map_err(err)?;
                let pooler = linear(cfg.hidden_size, cfg.hidden_size, vb.pp("bert.pooler.dense")).map_err(err)?;
                let classifier = linear(cfg.hidden_size, 1, vb.pp("classifier")).map_err(err)?;
                Cross::Bert { model: Box::new(model), pooler, classifier }
            }
            "xlmr-cross" => {
                let cfg: candle_transformers::models::xlm_roberta::Config = serde_json::from_str(cfg).map_err(|e| e.to_string())?;
                Cross::Xlmr(Box::new(candle_transformers::models::xlm_roberta::XLMRobertaForSequenceClassification::new(1, &cfg, vb).map_err(err)?))
            }
            "deberta-cross" => {
                let cfg: candle_transformers::models::debertav2::Config = serde_json::from_str(cfg).map_err(|e| e.to_string())?;
                Cross::Deberta(Box::new(candle_transformers::models::debertav2::DebertaV2SeqClassificationModel::load(vb, &cfg, None).map_err(err)?))
            }
            other => return Err(format!("unknown reranker kind {}", other)),
        };
        let mut tok = Tokenizer::from_bytes(file("tokenizer.json")?).map_err(|e| e.to_string())?;
        tok.with_padding(Some(PaddingParams::default()));
        // A long passage is cut, never the question.
        tok.with_truncation(Some(TruncationParams { max_length: header.meta.max_tokens, strategy: TruncationStrategy::OnlySecond, ..Default::default() })).map_err(|e| e.to_string())?;
        let threads = std::env::var("DELPHI_EMBED_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(if dev.is_cpu() { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 6) } else { 1 });
        // A package records its calibrated score floor where an embedding package records its similarity floor.
        let min_score = Some(header.meta.min_similarity).filter(|f| *f != 0.0);
        Ok(CandleReranker { id: header.meta.id, min_score, net, tok, dev, threads })
    }

    fn forward(&self, query: &str, docs: &[&String]) -> Result<Vec<f32>, String> {
        let pairs: Vec<EncodeInput> = docs.iter().map(|d| EncodeInput::Dual(query.to_string().into(), d.to_string().into())).collect();
        let enc = self.tok.encode_batch(pairs, true).map_err(|e| e.to_string())?;
        let (b, l) = (enc.len(), enc[0].get_ids().len());
        let cat = |f: &dyn Fn(&tokenizers::Encoding) -> Vec<u32>| -> Vec<u32> { enc.iter().flat_map(f).collect() };
        let (ids, types, mask) = (cat(&|e| e.get_ids().to_vec()), cat(&|e| e.get_type_ids().to_vec()), cat(&|e| e.get_attention_mask().to_vec()));
        let run = || -> candle_core::Result<Vec<f32>> {
            let ids = Tensor::from_vec(ids, (b, l), &self.dev)?;
            let types = Tensor::from_vec(types, (b, l), &self.dev)?;
            let mask = Tensor::from_vec(mask, (b, l), &self.dev)?;
            let logits = match &self.net {
                Cross::Bert { model, pooler, classifier } => {
                    let hidden = model.forward(&ids, &types, Some(&mask))?;
                    classifier.forward(&pooler.forward(&hidden.narrow(1, 0, 1)?.squeeze(1)?.contiguous()?)?.tanh()?)?
                }
                Cross::Xlmr(m) => m.forward(&ids, &mask, &types)?,
                Cross::Deberta(m) => m.forward(&ids, Some(types), Some(mask))?,
            };
            logits.to_dtype(DType::F32)?.flatten_all()?.to_vec1::<f32>()
        };
        run().map_err(|e| e.to_string())
    }
}

impl Reranker for CandleReranker {
    fn model_id(&self) -> &str {
        &self.id
    }

    fn min_score(&self) -> Option<f32> {
        self.min_score
    }

    fn score(&self, query: &str, docs: &[String]) -> Result<Vec<f32>, String> {
        // Longest first so a batch pads little; scores go back in the order of `docs`.
        let mut order: Vec<usize> = (0..docs.len()).collect();
        order.sort_by_key(|i| std::cmp::Reverse(docs[*i].len()));
        let size = if self.dev.is_cpu() { CPU_BATCH } else { GPU_BATCH };
        let batches: Vec<&[usize]> = order.chunks(size).collect();
        let next = AtomicUsize::new(0);
        let out: Mutex<Vec<f32>> = Mutex::new(vec![0.0; docs.len()]);
        let failure: Mutex<Option<String>> = Mutex::new(None);
        std::thread::scope(|s| {
            for _ in 0..self.threads.min(batches.len()).max(1) {
                s.spawn(|| {
                    while let Some(chunk) = batches.get(next.fetch_add(1, Ordering::SeqCst)) {
                        let batch: Vec<&String> = chunk.iter().map(|i| &docs[*i]).collect();
                        match self.forward(query, &batch) {
                            Ok(scores) => {
                                let mut o = out.lock().unwrap();
                                for (i, s) in chunk.iter().zip(scores) {
                                    o[*i] = s;
                                }
                            }
                            Err(e) => {
                                *failure.lock().unwrap() = Some(e);
                                break;
                            }
                        }
                    }
                });
            }
        });
        match failure.into_inner().unwrap() {
            Some(e) => Err(e),
            None => Ok(out.into_inner().unwrap()),
        }
    }
}

pub fn open_bytes(bytes: &[u8]) -> Result<Arc<dyn Reranker>, String> {
    Ok(Arc::new(CandleReranker::load(bytes)?))
}

pub fn open(path: &std::path::Path) -> Result<Arc<dyn Reranker>, String> {
    open_bytes(&std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::local::{pack, Meta};
    use candle_nn::VarMap;
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::pre_tokenizers::whitespace::Whitespace;
    use tokenizers::processors::template::TemplateProcessing;

    const CONFIG: &str = r#"{"vocab_size":32,"hidden_size":64,"num_hidden_layers":2,"num_attention_heads":4,"intermediate_size":128,"hidden_act":"gelu","hidden_dropout_prob":0.0,"max_position_embeddings":64,"type_vocab_size":2,"initializer_range":0.02,"layer_norm_eps":1e-12,"pad_token_id":0,"model_type":"bert"}"#;

    /// A tiny BERT cross-encoder (random weights, as `BertForSequenceClassification` names them) and a tokenizer
    /// that knows sentence pairs, written as a model directory. Returns the original model's scorer.
    fn tiny_cross(dir: &std::path::Path) -> impl Fn(&Tokenizer, &str, &str) -> f32 {
        let words = ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "retry", "ladder", "escalates", "zebra", "stripes", "pipe", "buffer", "deadlock", "how", "does", "the"];
        let model = WordLevel::builder().vocab(words.iter().enumerate().map(|(i, w)| (w.to_string(), i as u32)).collect()).unk_token("[UNK]".into()).build().unwrap();
        let mut tok = Tokenizer::new(model);
        tok.with_pre_tokenizer(Some(Whitespace {}));
        let pair = TemplateProcessing::builder().try_single("[CLS] $A [SEP]").unwrap().try_pair("[CLS] $A [SEP] $B:1 [SEP]:1").unwrap().special_tokens(vec![("[CLS]", 2), ("[SEP]", 3)]).build().unwrap();
        tok.with_post_processor(Some(pair));
        tok.save(dir.join("tokenizer.json"), false).unwrap();
        std::fs::write(dir.join("config.json"), CONFIG).unwrap();
        let vars = VarMap::new();
        let vb = VarBuilder::from_varmap(&vars, DType::F32, &Device::Cpu);
        let cfg: candle_transformers::models::bert::Config = serde_json::from_str(CONFIG).unwrap();
        let bert = candle_transformers::models::bert::BertModel::load(vb.pp("bert"), &cfg).unwrap();
        let pooler = linear(64, 64, vb.pp("bert.pooler.dense")).unwrap();
        let classifier = linear(64, 1, vb.pp("classifier")).unwrap();
        vars.save(dir.join("model.safetensors")).unwrap();
        move |tok: &Tokenizer, q: &str, d: &str| {
            let enc = tok.encode((q, d), true).unwrap();
            let n = enc.get_ids().len();
            let t = |v: &[u32]| Tensor::from_vec(v.to_vec(), (1, n), &Device::Cpu).unwrap();
            let hidden = bert.forward(&t(enc.get_ids()), &t(enc.get_type_ids()), Some(&t(enc.get_attention_mask()))).unwrap();
            let pooled = pooler.forward(&hidden.narrow(1, 0, 1).unwrap().squeeze(1).unwrap().contiguous().unwrap()).unwrap().tanh().unwrap();
            classifier.forward(&pooled).unwrap().flatten_all().unwrap().to_vec1::<f32>().unwrap()[0]
        }
    }

    fn meta() -> Meta {
        Meta { id: "tiny-cross-q8".into(), kind: "bert-cross".into(), pooling: String::new(), query_prefix: String::new(), doc_prefix: String::new(), max_tokens: 32, min_similarity: 0.0, dim: 1, source: "test".into() }
    }

    #[test]
    fn a_packed_cross_encoder_scores_passages_like_the_original_weights_to_within_quantization_error() {
        let d = tempfile::tempdir().unwrap();
        let reference = tiny_cross(d.path());
        let out = d.path().join("rr.dlphmdl");
        pack(d.path(), &meta(), &out).unwrap();
        let rr = CandleReranker::load(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(rr.model_id(), "tiny-cross-q8");
        let tok = Tokenizer::from_file(d.path().join("tokenizer.json")).unwrap();
        let q = "how does the retry ladder escalates";
        let docs: Vec<String> = ["retry ladder escalates", "zebra stripes", "pipe buffer deadlock the", "retry"].iter().map(|s| s.to_string()).collect();
        let scores = rr.score(q, &docs).unwrap();
        assert_eq!(scores.len(), 4);
        for (d, s) in docs.iter().zip(&scores) {
            let want = reference(&tok, q, d);
            assert!((s - want).abs() < 0.2, "{:?}: {} vs {}", d, s, want);
        }
        // The same passage scores the same alone as in a batch: order and batching do not matter.
        let alone = rr.score(q, &docs[1..2]).unwrap();
        assert!((alone[0] - scores[1]).abs() < 1e-4);
        assert!(rr.score(q, &[]).unwrap().is_empty());
    }

    #[test]
    fn the_score_floor_comes_from_the_package_when_it_was_calibrated() {
        let d = tempfile::tempdir().unwrap();
        let _reference = tiny_cross(d.path());
        let load = |floor: f32| {
            let out = d.path().join("rr.dlphmdl");
            pack(d.path(), &Meta { min_similarity: floor, ..meta() }, &out).unwrap();
            CandleReranker::load(&std::fs::read(&out).unwrap()).unwrap()
        };
        assert_eq!(load(-9.0).min_score(), Some(-9.0));
        assert_eq!(load(0.0).min_score(), None, "an uncalibrated package has no floor");
    }

    #[test]
    fn a_package_of_another_kind_is_refused_by_the_reranker() {
        let d = tempfile::tempdir().unwrap();
        let _reference = tiny_cross(d.path());
        let out = d.path().join("rr.dlphmdl");
        pack(d.path(), &Meta { kind: "mystery".into(), ..meta() }, &out).unwrap();
        assert!(CandleReranker::load(&std::fs::read(&out).unwrap()).is_err());
    }
}
