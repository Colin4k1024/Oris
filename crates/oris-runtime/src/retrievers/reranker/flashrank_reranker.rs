use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use log::{debug, info, warn};

use crate::error::RetrieverError;
use crate::schemas::{Document, Retriever};

#[cfg(feature = "flashrank")]
use ndarray::{Array2, Axis};
#[cfg(feature = "flashrank")]
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Value,
};
#[cfg(feature = "flashrank")]
use std::path::Path;
#[cfg(feature = "flashrank")]
use tokenizers::Tokenizer;

/// Configuration for FlashRank reranker.
#[derive(Debug, Clone)]
pub struct FlashRankRerankerConfig {
    /// Model label for logging and observability.
    pub model: String,
    /// Top K documents to return after reranking.
    pub top_k: Option<usize>,
    /// Path to local ONNX model file. Falls back to `ORIS_FLASHRANK_ONNX_MODEL_PATH`.
    pub onnx_model_path: Option<PathBuf>,
    /// Path to tokenizer file (`tokenizer.json`). Falls back to `ORIS_FLASHRANK_TOKENIZER_PATH`.
    pub tokenizer_path: Option<PathBuf>,
    /// Optional ONNX Runtime dynamic library path. Falls back to local Homebrew defaults.
    pub ort_dylib_path: Option<PathBuf>,
    /// Maximum token length for query-document pairs.
    pub onnx_max_length: usize,
    /// ONNX inference batch size.
    pub onnx_batch_size: usize,
}

impl Default for FlashRankRerankerConfig {
    fn default() -> Self {
        Self {
            model: "ms-marco-MiniLM-L-12-v2".to_string(),
            top_k: None,
            onnx_model_path: None,
            tokenizer_path: None,
            ort_dylib_path: None,
            onnx_max_length: 256,
            onnx_batch_size: 8,
        }
    }
}

trait FlashRankInferenceBackend: Send + Sync {
    fn infer_scores(
        &self,
        query: &str,
        documents: &[Document],
        config: &FlashRankRerankerConfig,
    ) -> Result<Vec<f64>, RetrieverError>;
}

/// FlashRank reranker backed by local ONNX Runtime inference with automatic fallback.
pub struct FlashRankReranker {
    base_retriever: Arc<dyn Retriever>,
    config: FlashRankRerankerConfig,
    inference_backend: Arc<dyn FlashRankInferenceBackend>,
}

impl FlashRankReranker {
    /// Create a new FlashRank reranker.
    pub fn new(base_retriever: Arc<dyn Retriever>) -> Self {
        Self::with_config(base_retriever, FlashRankRerankerConfig::default())
    }

    /// Create a new FlashRank reranker with custom config.
    pub fn with_config(
        base_retriever: Arc<dyn Retriever>,
        config: FlashRankRerankerConfig,
    ) -> Self {
        Self {
            base_retriever,
            config,
            inference_backend: default_flashrank_inference_backend(),
        }
    }

    #[cfg(test)]
    fn with_config_and_backend(
        base_retriever: Arc<dyn Retriever>,
        config: FlashRankRerankerConfig,
        inference_backend: Arc<dyn FlashRankInferenceBackend>,
    ) -> Self {
        Self {
            base_retriever,
            config,
            inference_backend,
        }
    }

    /// Fallback reranking based on keyword overlap.
    fn rerank_simple(&self, query: &str, documents: Vec<Document>) -> Vec<Document> {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let denominator = query_words.len().max(1) as f64;

        let mut scored: Vec<(Document, f64)> = documents
            .into_iter()
            .map(|mut doc| {
                let doc_lower = doc.page_content.to_lowercase();
                let score = query_words
                    .iter()
                    .map(|word| if doc_lower.contains(word) { 1.0 } else { 0.0 })
                    .sum::<f64>()
                    / denominator;
                doc.score = score;
                (doc, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

        let results: Vec<Document> = scored.into_iter().map(|(doc, _)| doc).collect();
        if let Some(k) = self.config.top_k {
            results.into_iter().take(k).collect()
        } else {
            results
        }
    }

    fn rerank_with_scores(
        &self,
        documents: Vec<Document>,
        scores: Vec<f64>,
    ) -> Result<Vec<Document>, RetrieverError> {
        if documents.len() != scores.len() {
            return Err(RetrieverError::RerankerError(format!(
                "score/document length mismatch: scores={}, documents={}",
                scores.len(),
                documents.len()
            )));
        }

        let mut scored: Vec<(Document, f64)> = documents
            .into_iter()
            .zip(scores)
            .map(|(mut doc, score)| {
                doc.score = score;
                (doc, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

        let results: Vec<Document> = scored.into_iter().map(|(doc, _)| doc).collect();
        if let Some(k) = self.config.top_k {
            Ok(results.into_iter().take(k).collect())
        } else {
            Ok(results)
        }
    }
}

#[cfg(feature = "flashrank")]
#[derive(Default)]
struct OnnxRuntimeInferenceBackend;

#[cfg(feature = "flashrank")]
impl OnnxRuntimeInferenceBackend {
    fn configure_ort_dylib_path(config: &FlashRankRerankerConfig) {
        if std::env::var_os("ORT_DYLIB_PATH").is_some() {
            return;
        }

        let mut candidates = Vec::new();
        if let Some(custom) = config.ort_dylib_path.clone() {
            candidates.push(custom);
        }
        candidates.push(PathBuf::from(
            "/opt/homebrew/opt/onnxruntime/lib/libonnxruntime.dylib",
        ));
        candidates.push(PathBuf::from(
            "/usr/local/opt/onnxruntime/lib/libonnxruntime.dylib",
        ));
        candidates.push(PathBuf::from("/usr/lib/libonnxruntime.so"));
        candidates.push(PathBuf::from("/usr/lib64/libonnxruntime.so"));

        for candidate in candidates {
            if candidate.exists() {
                std::env::set_var("ORT_DYLIB_PATH", &candidate);
                info!(
                    "flashrank configured local ONNX Runtime dylib from {}",
                    candidate.display()
                );
                return;
            }
        }
    }

    fn resolve_required_path(
        configured: &Option<PathBuf>,
        env_key: &str,
        label: &str,
    ) -> Result<PathBuf, RetrieverError> {
        let resolved = configured
            .clone()
            .or_else(|| std::env::var(env_key).ok().map(PathBuf::from))
            .ok_or_else(|| {
                RetrieverError::ConfigurationError(format!(
                    "{label} is required (set config or {env_key})"
                ))
            })?;

        if resolved.exists() {
            Ok(resolved)
        } else {
            Err(RetrieverError::ConfigurationError(format!(
                "{label} not found at {}",
                resolved.display()
            )))
        }
    }

    fn load_session(model_path: &Path) -> Result<Session, RetrieverError> {
        let mut builder = Session::builder().map_err(|err| {
            RetrieverError::RerankerError(format!("failed to initialize ONNX Runtime: {err}"))
        })?;
        builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|err| {
                RetrieverError::RerankerError(format!(
                    "failed to set ONNX graph optimization level: {err}"
                ))
            })?;
        if let Ok(threads) = std::thread::available_parallelism() {
            builder = builder.with_intra_threads(threads.get()).map_err(|err| {
                RetrieverError::RerankerError(format!("failed to configure ONNX threads: {err}"))
            })?;
        }
        builder.commit_from_file(model_path).map_err(|err| {
            RetrieverError::RerankerError(format!(
                "failed to load ONNX model {}: {err}",
                model_path.display()
            ))
        })
    }

    fn load_tokenizer(path: &Path) -> Result<Tokenizer, RetrieverError> {
        Tokenizer::from_file(path).map_err(|err| {
            RetrieverError::RerankerError(format!(
                "failed to load tokenizer {}: {err}",
                path.display()
            ))
        })
    }

    fn infer_batch(
        session: &Session,
        tokenizer: &Tokenizer,
        query: &str,
        documents: &[Document],
        max_length: usize,
    ) -> Result<Vec<f64>, RetrieverError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        let max_length = max_length.max(8);
        let encoded_inputs: Vec<(&str, &str)> = documents
            .iter()
            .map(|doc| (query, doc.page_content.as_str()))
            .collect();
        let encodings = tokenizer
            .encode_batch(encoded_inputs, true)
            .map_err(|err| {
                RetrieverError::RerankerError(format!(
                    "tokenization failed for reranker batch: {err}"
                ))
            })?;

        let sequence_len = encodings
            .iter()
            .map(|encoding| encoding.len().min(max_length))
            .max()
            .unwrap_or(1)
            .max(1);
        let batch_size = encodings.len();

        let mut input_ids = vec![0_i64; batch_size * sequence_len];
        let mut attention_mask = vec![0_i64; batch_size * sequence_len];
        let mut token_type_ids = vec![0_i64; batch_size * sequence_len];

        for (row, encoding) in encodings.iter().enumerate() {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let type_ids = encoding.get_type_ids();
            let usable = encoding.len().min(sequence_len);

            for col in 0..usable {
                let idx = row * sequence_len + col;
                input_ids[idx] = i64::from(*ids.get(col).unwrap_or(&0));
                attention_mask[idx] = i64::from(*mask.get(col).unwrap_or(&1));
                token_type_ids[idx] = i64::from(*type_ids.get(col).unwrap_or(&0));
            }
        }

        let input_ids_array = Array2::from_shape_vec((batch_size, sequence_len), input_ids)
            .map_err(|err| {
                RetrieverError::RerankerError(format!(
                    "failed to shape ONNX input_ids tensor: {err}"
                ))
            })?;
        let attention_mask_array =
            Array2::from_shape_vec((batch_size, sequence_len), attention_mask).map_err(|err| {
                RetrieverError::RerankerError(format!(
                    "failed to shape ONNX attention_mask tensor: {err}"
                ))
            })?;
        let token_type_ids_array =
            Array2::from_shape_vec((batch_size, sequence_len), token_type_ids).map_err(|err| {
                RetrieverError::RerankerError(format!(
                    "failed to shape ONNX token_type_ids tensor: {err}"
                ))
            })?;

        let mut session_inputs = Vec::with_capacity(session.inputs.len());
        for (index, input) in session.inputs.iter().enumerate() {
            let input_name = input.name.clone();
            let lower = input_name.to_ascii_lowercase();
            let value = if lower.contains("attention_mask") {
                Value::from_array(attention_mask_array.clone())
            } else if lower.contains("token_type_ids") {
                Value::from_array(token_type_ids_array.clone())
            } else if lower.contains("input_ids") {
                Value::from_array(input_ids_array.clone())
            } else {
                match index {
                    0 => Value::from_array(input_ids_array.clone()),
                    1 => Value::from_array(attention_mask_array.clone()),
                    _ => Value::from_array(token_type_ids_array.clone()),
                }
            }
            .map_err(|err| {
                RetrieverError::RerankerError(format!(
                    "failed to materialize ONNX input tensor '{input_name}': {err}"
                ))
            })?;
            session_inputs.push((input_name, value));
        }

        if session_inputs.is_empty() {
            return Err(RetrieverError::RerankerError(
                "ONNX model has no inputs".to_string(),
            ));
        }

        let outputs = session.run(session_inputs).map_err(|err| {
            RetrieverError::RerankerError(format!("ONNX inference execution failed: {err}"))
        })?;
        if outputs.len() == 0 {
            return Err(RetrieverError::RerankerError(
                "ONNX inference produced no outputs".to_string(),
            ));
        }

        let logits = if outputs.contains_key("logits") {
            outputs["logits"].try_extract_tensor::<f32>()
        } else {
            outputs[0].try_extract_tensor::<f32>()
        }
        .map_err(|err| {
            RetrieverError::RerankerError(format!("failed to extract ONNX logits tensor: {err}"))
        })?;

        let mut scores = if logits.ndim() == 2 && logits.shape()[0] == documents.len() {
            (0..documents.len())
                .map(|row| {
                    logits
                        .index_axis(Axis(0), row)
                        .iter()
                        .next()
                        .copied()
                        .unwrap_or(0.0) as f64
                })
                .collect::<Vec<_>>()
        } else {
            logits.iter().map(|value| *value as f64).collect::<Vec<_>>()
        };

        if scores.len() < documents.len() {
            return Err(RetrieverError::RerankerError(format!(
                "ONNX output score count mismatch: got {}, expected {}",
                scores.len(),
                documents.len()
            )));
        }
        scores.truncate(documents.len());
        Ok(scores)
    }
}

#[cfg(feature = "flashrank")]
impl FlashRankInferenceBackend for OnnxRuntimeInferenceBackend {
    fn infer_scores(
        &self,
        query: &str,
        documents: &[Document],
        config: &FlashRankRerankerConfig,
    ) -> Result<Vec<f64>, RetrieverError> {
        Self::configure_ort_dylib_path(config);
        let model_path = Self::resolve_required_path(
            &config.onnx_model_path,
            "ORIS_FLASHRANK_ONNX_MODEL_PATH",
            "flashrank ONNX model path",
        )?;
        let tokenizer_path = Self::resolve_required_path(
            &config.tokenizer_path,
            "ORIS_FLASHRANK_TOKENIZER_PATH",
            "flashrank tokenizer path",
        )?;

        debug!(
            "flashrank ONNX inference start model={} model_path={} tokenizer_path={} docs={}",
            config.model,
            model_path.display(),
            tokenizer_path.display(),
            documents.len()
        );

        let session = Self::load_session(&model_path)?;
        let tokenizer = Self::load_tokenizer(&tokenizer_path)?;
        let mut scores = Vec::with_capacity(documents.len());
        let batch_size = config.onnx_batch_size.max(1);

        for batch in documents.chunks(batch_size) {
            scores.extend(Self::infer_batch(
                &session,
                &tokenizer,
                query,
                batch,
                config.onnx_max_length,
            )?);
        }

        if scores.len() != documents.len() {
            return Err(RetrieverError::RerankerError(format!(
                "ONNX score count mismatch across batches: got {}, expected {}",
                scores.len(),
                documents.len()
            )));
        }
        Ok(scores)
    }
}

#[cfg(not(feature = "flashrank"))]
#[derive(Default)]
struct FeatureDisabledOnnxInferenceBackend;

#[cfg(not(feature = "flashrank"))]
impl FlashRankInferenceBackend for FeatureDisabledOnnxInferenceBackend {
    fn infer_scores(
        &self,
        _query: &str,
        _documents: &[Document],
        _config: &FlashRankRerankerConfig,
    ) -> Result<Vec<f64>, RetrieverError> {
        Err(RetrieverError::ConfigurationError(
            "flashrank feature is disabled; ONNX Runtime inference is unavailable".to_string(),
        ))
    }
}

#[cfg(feature = "flashrank")]
fn default_flashrank_inference_backend() -> Arc<dyn FlashRankInferenceBackend> {
    Arc::new(OnnxRuntimeInferenceBackend)
}

#[cfg(not(feature = "flashrank"))]
fn default_flashrank_inference_backend() -> Arc<dyn FlashRankInferenceBackend> {
    Arc::new(FeatureDisabledOnnxInferenceBackend)
}

#[async_trait]
impl Retriever for FlashRankReranker {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError> {
        let documents = self.base_retriever.get_relevant_documents(query).await?;
        if documents.is_empty() {
            return Ok(documents);
        }

        match self
            .inference_backend
            .infer_scores(query, &documents, &self.config)
        {
            Ok(scores) => match self.rerank_with_scores(documents.clone(), scores) {
                Ok(reranked) => {
                    info!(
                        "flashrank ONNX rerank succeeded model={} docs={}",
                        self.config.model,
                        reranked.len()
                    );
                    return Ok(reranked);
                }
                Err(err) => {
                    warn!(
                        "flashrank ONNX rerank score mapping failed, fallback enabled: {}",
                        err
                    );
                }
            },
            Err(err) => {
                warn!(
                    "flashrank ONNX rerank unavailable, fallback enabled: {}",
                    err
                );
            }
        }

        debug!("flashrank reranker using keyword-overlap fallback path");
        Ok(self.rerank_simple(query, documents))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    struct StaticRetriever {
        docs: Vec<Document>,
    }

    #[async_trait]
    impl Retriever for StaticRetriever {
        async fn get_relevant_documents(
            &self,
            _query: &str,
        ) -> Result<Vec<Document>, RetrieverError> {
            Ok(self.docs.clone())
        }
    }

    enum MockInferenceBehavior {
        Scores(Vec<f64>),
        Error(String),
    }

    struct MockInferenceBackend {
        behavior: MockInferenceBehavior,
    }

    impl FlashRankInferenceBackend for MockInferenceBackend {
        fn infer_scores(
            &self,
            _query: &str,
            _documents: &[Document],
            _config: &FlashRankRerankerConfig,
        ) -> Result<Vec<f64>, RetrieverError> {
            match &self.behavior {
                MockInferenceBehavior::Scores(scores) => Ok(scores.clone()),
                MockInferenceBehavior::Error(message) => {
                    Err(RetrieverError::ConfigurationError(message.clone()))
                }
            }
        }
    }

    fn doc(content: &str) -> Document {
        Document::new(content)
    }

    #[tokio::test]
    async fn flashrank_model_missing_falls_back_to_simple_rerank() {
        let retriever = Arc::new(StaticRetriever {
            docs: vec![doc("rust async runtime"), doc("vector database indexing")],
        });
        let reranker = FlashRankReranker::with_config_and_backend(
            retriever,
            FlashRankRerankerConfig::default(),
            Arc::new(MockInferenceBackend {
                behavior: MockInferenceBehavior::Error("missing ONNX model path".to_string()),
            }),
        );

        let ranked = reranker
            .get_relevant_documents("rust runtime")
            .await
            .expect("fallback rerank should succeed");

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].page_content, "rust async runtime");
        assert!(ranked[0].score >= ranked[1].score);
    }

    #[tokio::test]
    async fn flashrank_inference_success_uses_onnx_scores() {
        let retriever = Arc::new(StaticRetriever {
            docs: vec![doc("doc-a"), doc("doc-b"), doc("doc-c")],
        });
        let reranker = FlashRankReranker::with_config_and_backend(
            retriever,
            FlashRankRerankerConfig {
                top_k: Some(2),
                ..Default::default()
            },
            Arc::new(MockInferenceBackend {
                behavior: MockInferenceBehavior::Scores(vec![0.2, 0.9, 0.4]),
            }),
        );

        let ranked = reranker
            .get_relevant_documents("irrelevant")
            .await
            .expect("onnx rerank should succeed");

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].page_content, "doc-b");
        assert_eq!(ranked[1].page_content, "doc-c");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[tokio::test]
    async fn flashrank_inference_failure_falls_back() {
        let retriever = Arc::new(StaticRetriever {
            docs: vec![doc("flower orchid care"), doc("database tuning notes")],
        });
        let reranker = FlashRankReranker::with_config_and_backend(
            retriever,
            FlashRankRerankerConfig::default(),
            Arc::new(MockInferenceBackend {
                behavior: MockInferenceBehavior::Error(
                    "onnx inference execution failed".to_string(),
                ),
            }),
        );

        let ranked = reranker
            .get_relevant_documents("orchid")
            .await
            .expect("fallback rerank should succeed");

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].page_content, "flower orchid care");
        assert!(ranked[0].score >= ranked[1].score);
    }
}
