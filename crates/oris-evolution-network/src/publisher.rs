//! `NetworkPublisher` — DIP abstraction for pushing an [`EvolutionEnvelope`] to a remote endpoint.

use async_trait::async_trait;
use thiserror::Error;

use crate::EvolutionEnvelope;

/// Errors returned by [`NetworkPublisher::publish_envelope`].
#[derive(Debug, Error)]
pub enum NetworkPublishError {
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("signing key not configured — cannot produce Ed25519 signature")]
    SigningKeyNotConfigured,
}

/// Abstraction for publishing an [`EvolutionEnvelope`] to a remote endpoint.
///
/// Implement this trait to inject a custom publish strategy into [`EvoKernel`].
/// Failures should be treated as non-fatal by callers — log a warning and
/// continue without aborting the promotion path.
#[async_trait]
pub trait NetworkPublisher: Send + Sync {
    /// Publish an evolution envelope to the remote network endpoint.
    async fn publish_envelope(
        &self,
        envelope: &EvolutionEnvelope,
    ) -> Result<(), NetworkPublishError>;
}
