//! ONNX Runtime cross-encoder reranker.
//!
//! Scores `(query, candidate)` pairs jointly and returns a relevance
//! probability per candidate. Loads `cross-encoder/ms-marco-TinyBERT-L-2-v2`
//! via `ort` with dynamic linking. Degrades gracefully when the ONNX
//! Runtime library or model files are unavailable — search simply
//! skips the rerank stage.

use std::path::PathBuf;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use tracing::warn;

use super::model::{EmbeddingError, EmbeddingResult, RerankModel};
use super::registry;

/// Default reranker model ID.
const DEFAULT_RERANKER_MODEL: &str = registry::DEFAULT_RERANKER_MODEL;

/// Maximum characters of candidate text fed to the model. The
/// tokenizer truncates at 512 tokens regardless; this bound keeps a
/// pathological entity from dominating encode time.
const CANDIDATE_CHAR_CAP: usize = 4000;

/// ONNX Runtime cross-encoder for candidate reranking.
///
/// Loads the model and tokenizer lazily on first use. All candidates
/// of one `score_batch` call are encoded and scored in a single
/// inference pass — one padded batch is much cheaper than N separate
/// forward passes.
pub struct OnnxRerankModel {
    model_id: String,
    model_dir: PathBuf,
    session: Mutex<Option<Session>>,
    tokenizer: Mutex<Option<tokenizers::Tokenizer>>,
    available: Mutex<bool>,
    idle_tracker: super::resources::IdleTracker,
    min_free_mb: u64,
    /// Whether the loaded session declares a `token_type_ids` input.
    /// BERT-family cross-encoders need it; DeBERTa exports reject it.
    has_token_type_ids: Mutex<bool>,
}

impl OnnxRerankModel {
    /// Create a new reranker. `model_id` selects the model directory
    /// under `models_base`. Empty string uses the default.
    pub fn new(models_base: &std::path::Path, model_id: &str) -> Self {
        Self::with_resource_config(models_base, model_id, 0, 0)
    }

    /// Create with resource-aware settings.
    pub fn with_resource_config(
        models_base: &std::path::Path,
        model_id: &str,
        idle_ttl_secs: u64,
        min_free_mb: u64,
    ) -> Self {
        let id = if model_id.is_empty() {
            DEFAULT_RERANKER_MODEL
        } else {
            model_id
        };
        Self {
            model_id: id.to_string(),
            model_dir: models_base.join(id),
            session: Mutex::new(None),
            tokenizer: Mutex::new(None),
            available: Mutex::new(false),
            idle_tracker: super::resources::IdleTracker::new(idle_ttl_secs),
            min_free_mb,
            has_token_type_ids: Mutex::new(false),
        }
    }

    /// Drop the loaded model and tokenizer, freeing memory.
    pub fn unload(&self) {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.tokenizer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        *self.available.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }

    /// Find the ONNX model file. Registered model IDs resolve their
    /// layout exactly — required because quality variants of one
    /// model share a snapshot dir (e.g. `model_quantized.onnx` and
    /// `model_q4.onnx` coexisting). Unknown IDs fall back to probing
    /// all known layouts.
    fn find_onnx_file(&self) -> Option<PathBuf> {
        if let Some(entry) = registry::lookup(&self.model_id) {
            let exact = self
                .model_dir
                .join(registry::onnx_relative_path(entry.onnx_layout));
            if exact.exists() {
                return Some(exact);
            }
        }
        [
            self.model_dir.join("onnx").join("model_quint8_avx2.onnx"),
            self.model_dir.join("onnx").join("model_quantized.onnx"),
            self.model_dir.join("onnx").join("model_q4.onnx"),
            self.model_dir.join("onnx").join("model.onnx"),
            self.model_dir.join("model.onnx"),
        ]
        .into_iter()
        .find(|p| p.exists())
    }

    fn try_load(&self) -> EmbeddingResult<()> {
        if self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
        {
            self.idle_tracker.touch();
            return Ok(());
        }

        if self.idle_tracker.is_idle() {
            self.unload();
        }

        if !super::resources::has_enough_memory(self.min_free_mb) {
            return Err(EmbeddingError::ModelUnavailable(format!(
                "insufficient free memory (need >= {} MB)",
                self.min_free_mb
            )));
        }

        if !super::runtime::ensure_ort() {
            return Err(EmbeddingError::ModelUnavailable(
                "ONNX Runtime library not available".to_string(),
            ));
        }

        let Some(model_path) = self.find_onnx_file() else {
            return Err(EmbeddingError::ModelUnavailable(format!(
                "reranker model file not found under: {}",
                self.model_dir.display()
            )));
        };

        let tokenizer_path = self.model_dir.join("tokenizer.json");
        if !tokenizer_path.exists() {
            return Err(EmbeddingError::ModelUnavailable(format!(
                "reranker tokenizer file not found: {}",
                tokenizer_path.display()
            )));
        }

        let session = {
            crate::embed::suppress::suppress_stderr_during(|| {
                let mut builder = Session::builder().map_err(|e| {
                    warn!("reranker session builder failed: {}", e);
                    EmbeddingError::ModelUnavailable(format!("session builder: {}", e))
                })?;
                builder = builder
                    .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
                    .map_err(|e| {
                        warn!("reranker optimization level failed: {}", e);
                        EmbeddingError::ModelUnavailable(format!("optimization level: {}", e))
                    })?;
                builder.commit_from_file(&model_path).map_err(|e| {
                    warn!("reranker session load failed: {}", e);
                    EmbeddingError::ModelUnavailable(format!(
                        "failed to load reranker model: {}",
                        e
                    ))
                })
            })?
        };

        *self
            .has_token_type_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = session
            .inputs()
            .iter()
            .any(|i| i.name() == "token_type_ids");

        let mut tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            warn!("reranker tokenizer load failed: {}", e);
            EmbeddingError::ModelUnavailable(format!("failed to load reranker tokenizer: {}", e))
        })?;
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: 512,
                ..Default::default()
            }))
            .map_err(|e| {
                warn!("reranker tokenizer truncation setup failed: {}", e);
                EmbeddingError::ModelUnavailable(format!("truncation setup: {}", e))
            })?;
        // Pad to the longest sequence in each batch — required to stack
        // per-candidate encodings into one tensor.
        tokenizer.with_padding(Some(tokenizers::PaddingParams::default()));

        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
        *self.tokenizer.lock().unwrap_or_else(|e| e.into_inner()) = Some(tokenizer);
        *self.available.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.idle_tracker.touch();
        Ok(())
    }
}

impl RerankModel for OnnxRerankModel {
    fn score_batch(&self, query: &str, candidates: &[&str]) -> EmbeddingResult<Vec<f32>> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        self.try_load()?;

        let mut session_guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let mut tokenizer_guard = self.tokenizer.lock().unwrap_or_else(|e| e.into_inner());
        if session_guard.is_none() || tokenizer_guard.is_none() {
            // A concurrent try_load can unload the model (idle TTL)
            // between our try_load and this lock. Reload once; fail
            // loudly if it is still gone.
            drop(tokenizer_guard);
            drop(session_guard);
            self.try_load()?;
            session_guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
            tokenizer_guard = self.tokenizer.lock().unwrap_or_else(|e| e.into_inner());
        }
        let Some(session) = session_guard.as_mut() else {
            return Err(EmbeddingError::ModelUnavailable(
                "model unloaded during score_batch".to_string(),
            ));
        };
        let session: &mut Session = session;
        let Some(tokenizer) = tokenizer_guard.as_ref() else {
            return Err(EmbeddingError::ModelUnavailable(
                "tokenizer unloaded during score_batch".to_string(),
            ));
        };
        let wants_type_ids = *self
            .has_token_type_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let inputs: Vec<tokenizers::EncodeInput> = candidates
            .iter()
            .map(|c| {
                let capped: &str = if c.len() > CANDIDATE_CHAR_CAP {
                    let mut end = CANDIDATE_CHAR_CAP;
                    while !c.is_char_boundary(end) {
                        end -= 1;
                    }
                    &c[..end]
                } else {
                    c
                };
                tokenizers::EncodeInput::from((query, capped))
            })
            .collect();
        let encodings = tokenizer
            .encode_batch(inputs, true)
            .map_err(|e| EmbeddingError::InferenceFailed(format!("rerank tokenization: {}", e)))?;

        let batch = encodings.len() as i64;
        let seq_len = encodings[0].get_ids().len() as i64;
        let flat = |f: &dyn Fn(&tokenizers::Encoding) -> &[u32]| -> Vec<i64> {
            encodings
                .iter()
                .flat_map(|e| f(e).iter().map(|&v| v as i64))
                .collect()
        };
        let input_ids = Tensor::from_array((vec![batch, seq_len], flat(&|e| e.get_ids())))
            .map_err(|e| EmbeddingError::InferenceFailed(format!("rerank input tensor: {}", e)))?;
        let attention_mask =
            Tensor::from_array((vec![batch, seq_len], flat(&|e| e.get_attention_mask()))).map_err(
                |e| EmbeddingError::InferenceFailed(format!("rerank mask tensor: {}", e)),
            )?;
        let type_ids = Tensor::from_array((vec![batch, seq_len], flat(&|e| e.get_type_ids())))
            .map_err(|e| EmbeddingError::InferenceFailed(format!("rerank type tensor: {}", e)))?;

        let outputs = if wants_type_ids {
            session.run(ort::inputs![
                "input_ids" => input_ids,
                "attention_mask" => attention_mask,
                "token_type_ids" => type_ids,
            ])
        } else {
            session.run(ort::inputs![input_ids, attention_mask])
        }
        .map_err(|e| EmbeddingError::InferenceFailed(format!("rerank inference: {}", e)))?;

        let (_shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| EmbeddingError::InferenceFailed(format!("rerank extract: {}", e)))?;
        if data.len() < candidates.len() {
            return Err(EmbeddingError::InferenceFailed(format!(
                "rerank output has {} values, expected {}",
                data.len(),
                candidates.len()
            )));
        }

        // Raw logit -> probability. Only ordering matters downstream,
        // but a calibrated score is also exposed as the relevance.
        Ok(data[..candidates.len()]
            .iter()
            .map(|&logit| 1.0 / (1.0 + (-logit).exp()))
            .collect())
    }

    fn model_name(&self) -> &str {
        &self.model_id
    }

    fn is_available(&self) -> bool {
        *self.available.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn model_files_exist(&self) -> bool {
        self.find_onnx_file().is_some() && self.model_dir.join("tokenizer.json").exists()
    }
}
