use crate::langgraph::state::State;

use super::error::PersistenceError;

/// Trait for serializing and deserializing state
///
/// Serializers are used by checkpointers to save and load state.
pub trait Serializer<S: State>: Send + Sync {
    /// Serialize state to bytes
    fn serialize(&self, state: &S) -> Result<Vec<u8>, PersistenceError>;

    /// Deserialize state from bytes
    fn deserialize(&self, data: &[u8]) -> Result<S, PersistenceError>;
}

/// JSON-based serializer using serde_json
///
/// This is the default serializer, similar to Python's JsonPlusSerializer.
/// It handles common types including LangChain primitives.
pub struct JsonSerializer;

impl Default for JsonSerializer {
    fn default() -> Self {
        Self
    }
}

// Note: Serializer implementation is done per-state-type
// This avoids the unconstrained type parameter issue
