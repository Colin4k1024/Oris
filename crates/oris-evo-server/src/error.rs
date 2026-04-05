//! Error types for the Evolution Server

use thiserror::Error;

/// Result type alias
pub type Result<T> = std::result::Result<T, Error>;

/// Evolution Server error types
#[derive(Error, Debug)]
pub enum Error {
    /// IPC communication error
    #[error("IPC error: {0}")]
    Ipc(String),

    /// JSON serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Evolution pipeline error
    #[error("Pipeline error: {0}")]
    Pipeline(String),

    /// Gene not found
    #[error("Gene not found: {0}")]
    GeneNotFound(String),

    /// Validation failed
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Sandbox error
    #[error("Sandbox error: {0}")]
    Sandbox(String),

    /// Signature verification failed
    #[error("Signature verification failed: {0}")]
    Signature(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Shutdown requested
    #[error("Server shutdown requested")]
    Shutdown,
}

impl Error {
    /// Get the error code for JSON-RPC error responses
    pub fn code(&self) -> i32 {
        match self {
            Error::Ipc(_) => -32000,
            Error::Serialization(_) => -32700,
            Error::Pipeline(_) => -32000,
            Error::GeneNotFound(_) => -32001,
            Error::Validation(_) => -32002,
            Error::Sandbox(_) => -32003,
            Error::Signature(_) => -32004,
            Error::Database(_) => -32005,
            Error::Config(_) => -32006,
            Error::Io(_) => -32007,
            Error::Shutdown => -32008,
        }
    }
}
