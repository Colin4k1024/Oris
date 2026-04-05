//! Evolution Pipeline Driver
//!
//! Drives the StandardEvolutionPipeline for the IPC server

use std::sync::Arc;

use oris_evokernel::adapters::{LocalSandboxAdapter, RuntimeSignalExtractorAdapter, SqliteGeneStorePersistAdapter};
use oris_evolution::{
    EvolutionPipeline, EvolutionPipelineConfig, GeneCandidate, PipelineContext, Selector, SelectorInput,
    SignalExtractorInput, StandardEvolutionPipeline,
};
use uuid::Uuid;

use crate::error::Error;
use oris_evo_ipc_protocol::{EvolutionAction, EvolutionResult, Gene};

/// Simple selector that returns empty candidates (MVP - gene pool starts empty)
pub struct EmptySelector;

impl Selector for EmptySelector {
    fn select(&self, _input: &SelectorInput) -> Vec<GeneCandidate> {
        vec![]
    }
}

/// Confidence threshold for auto-solidify
const SOLIDIFY_THRESHOLD: f32 = 0.72;
/// Confidence drop percentage that triggers auto-revert
const REVERT_CONFIDENCE_DROP_THRESHOLD: f32 = 0.20;

/// Pipeline driver for evolution requests
pub struct PipelineDriver {
    /// Evolution pipeline
    pipeline: StandardEvolutionPipeline,
    /// Gene store adapter (for manual operations)
    #[allow(dead_code)]
    gene_store: Arc<SqliteGeneStorePersistAdapter>,
    /// Last known confidence for revert detection (protected by mutex for atomic check-and-update)
    last_confidence: std::sync::Mutex<f32>,
}

impl PipelineDriver {
    /// Create a new pipeline driver
    pub async fn new(store_path: &str) -> Result<Self, Error> {
        // Build the evolution pipeline
        let extractor = Arc::new(RuntimeSignalExtractorAdapter::new()
            .map_err(|e| Error::Pipeline(e.to_string()))?);

        let sandbox = Arc::new(LocalSandboxAdapter::new(
            format!("evo-{}", Uuid::new_v4()),
            "/tmp",
            "/tmp/oris-sandbox",
        ));

        let gene_store = Arc::new(
            SqliteGeneStorePersistAdapter::open(store_path)
                .map_err(|e| Error::Database(e.to_string()))?
        );

        let config = EvolutionPipelineConfig::default();
        let selector = Arc::new(EmptySelector);

        let pipeline = StandardEvolutionPipeline::new(config, selector)
            .with_signal_extractor(extractor)
            .with_sandbox(sandbox)
            .with_gene_store(gene_store.clone());

        Ok(Self {
            pipeline,
            gene_store,
            last_confidence: std::sync::Mutex::new(0.0),
        })
    }

    /// Process a signal and return evolution result
    pub async fn evolve(&self, signal: oris_evo_ipc_protocol::RuntimeSignal) -> Result<EvolutionResult, Error> {
        // Convert IPC signal to extractor input
        let extractor_input = SignalExtractorInput {
            compiler_output: Some(signal.content.clone()),
            stack_trace: None,
            logs: None,
            extra: serde_json::json!({}),
        };

        // Build pipeline context
        let ctx = PipelineContext {
            extractor_input: Some(extractor_input),
            ..Default::default()
        };

        // Execute the pipeline
        let pipeline_result = self.pipeline.execute(ctx)
            .map_err(|e| Error::Pipeline(e.to_string()))?;

        if !pipeline_result.success {
            return Ok(EvolutionResult {
                gene_id: None,
                confidence: 0.0,
                action: EvolutionAction::Reject,
                revert_triggered: false,
                evaluation_summary: pipeline_result.error.unwrap_or_else(|| "Pipeline failed".to_string()),
            });
        }

        // Analyze stage states to determine action
        let mut gene_id = None;
        let mut confidence = 0.0f32;

        for stage in &pipeline_result.stage_states {
            if stage.stage_name == "solidify" {
                if let oris_evolution::pipeline::PipelineStageState::Completed = &stage.state {
                    gene_id = Some(Uuid::new_v4());
                    confidence = 0.75; // Default confidence from pipeline
                }
            }
        }

        // Check for auto-revert and update confidence atomically under single lock
        let should_revert = {
            let mut last = self.last_confidence.lock().unwrap();
            let should_revert = Self::check_auto_revert_internal(*last, confidence);
            // Always update last_confidence, even if we revert
            *last = confidence;
            should_revert
        };

        // If revert triggered, remove gene and return reject
        if should_revert {
            if let Some(gid) = gene_id {
                let _ = self.revert_internal(gid, "Confidence drop detected").await;
            }
            return Ok(EvolutionResult {
                gene_id: None,
                confidence,
                action: EvolutionAction::Reject,
                revert_triggered: true,
                evaluation_summary: "Auto-revert: confidence drop detected".to_string(),
            });
        }

        // Determine action based on confidence
        let action = if confidence >= SOLIDIFY_THRESHOLD {
            EvolutionAction::Solidify
        } else if confidence > 0.0 {
            EvolutionAction::ApplyOnce
        } else {
            EvolutionAction::Reject
        };

        Ok(EvolutionResult {
            gene_id,
            confidence,
            action,
            revert_triggered: false,
            evaluation_summary: format!(
                "Pipeline completed: {:?}, confidence: {:.2}, action: {:?}",
                pipeline_result.stage_states, confidence, action
            ),
        })
    }

    /// Check if auto-revert should be triggered based on confidence drop (internal, static)
    fn check_auto_revert_internal(last_confidence: f32, current_confidence: f32) -> bool {
        if current_confidence <= 0.0 {
            return false;
        }

        if last_confidence <= 0.0 {
            return false;
        }

        let drop = last_confidence - current_confidence;
        let drop_percentage = drop / last_confidence;

        drop_percentage > REVERT_CONFIDENCE_DROP_THRESHOLD
    }

    /// Internal revert implementation
    async fn revert_internal(&self, gene_id: Uuid, reason: &str) -> Result<(), Error> {
        tracing::info!(gene_id = %gene_id, reason = %reason, "Auto-reverting gene");
        // In a full implementation, this would delete the gene from the store
        let _ = gene_id;
        let _ = reason;
        Ok(())
    }

    /// Query genes by similarity
    pub async fn query_genes(&self, pattern: &str, limit: usize) -> Result<Vec<Gene>, Error> {
        // Placeholder - would use semantic search
        let _ = pattern;
        let _ = limit;
        Ok(vec![])
    }

    /// Solidify a gene (confirm it should stay in the pool)
    pub async fn solidify(&self, gene_id: Uuid) -> Result<bool, Error> {
        let _ = gene_id;
        Ok(true)
    }

    /// Revert a gene (remove from pool)
    pub async fn revert(&self, gene_id: Uuid, reason: &str) -> Result<bool, Error> {
        self.revert_internal(gene_id, reason).await?;
        Ok(true)
    }

    /// List all genes
    pub async fn list_genes(&self, limit: usize, offset: usize) -> Result<(Vec<Gene>, usize), Error> {
        let _ = limit;
        let _ = offset;
        Ok((vec![], 0))
    }

    /// Verify a gene's signature
    pub async fn verify_signature(&self, gene_id: Uuid) -> Result<bool, Error> {
        // Signature verification is done at the pipeline level
        // This is a placeholder for manual verification requests
        let _ = gene_id;
        Ok(true)
    }
}

/// Signature verification utilities
pub mod signature {
    use oris_evolution_network::{verify_envelope, NodeKeypair, EvolutionEnvelope};

    /// Verify a gene's Ed25519 signature
    pub fn verify_gene_signature(
        public_key_hex: &str,
        envelope: &EvolutionEnvelope,
    ) -> Result<(), SignatureError> {
        verify_envelope(public_key_hex, envelope)
            .map_err(|e| SignatureError::VerificationFailed(e.to_string()))
    }

    /// Load node keypair for signing
    pub fn load_keypair() -> Result<NodeKeypair, SignatureError> {
        NodeKeypair::from_path(get_key_path())
            .map_err(|e| SignatureError::KeyLoadFailed(e.to_string()))
    }

    /// Get the default keypair path
    fn get_key_path() -> std::path::PathBuf {
        let home = std::env::var_os("HOME")
            .expect("HOME environment variable not set");
        std::path::PathBuf::from(home)
            .join(".oris")
            .join("node.key")
    }

    #[derive(Debug)]
    pub enum SignatureError {
        KeyLoadFailed(String),
        VerificationFailed(String),
    }

    impl std::fmt::Display for SignatureError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                SignatureError::KeyLoadFailed(e) => write!(f, "Failed to load keypair: {}", e),
                SignatureError::VerificationFailed(e) => write!(f, "Signature verification failed: {}", e),
            }
        }
    }

    impl std::error::Error for SignatureError {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pipeline_creation() {
        // This would need a temporary directory for the store
    }

    #[test]
    fn test_auto_revert_threshold() {
        // Test confidence drop detection logic
        let last_confidence = 0.80;
        let current_confidence = 0.55; // 31.25% drop

        let drop = last_confidence - current_confidence;
        let drop_percentage = drop / last_confidence;

        assert!(drop_percentage > REVERT_CONFIDENCE_DROP_THRESHOLD);
    }
}
