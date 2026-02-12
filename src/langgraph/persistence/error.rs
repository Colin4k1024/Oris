use thiserror::Error;

/// Errors that can occur when working with persistence
#[derive(Error, Debug)]
pub enum PersistenceError {
    #[error("Checkpoint not found: {0}")]
    CheckpointNotFound(String),

    #[error("Thread not found: {0}")]
    ThreadNotFound(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Invalid config: {0}")]
    InvalidConfig(String),

    #[error("Store error: {0}")]
    StoreError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[cfg(feature = "sqlite-persistence")]
impl From<rusqlite::Error> for PersistenceError {
    fn from(e: rusqlite::Error) -> Self {
        PersistenceError::DatabaseError(e.to_string())
    }
}

pub type PersistenceResult<T> = Result<T, PersistenceError>;
