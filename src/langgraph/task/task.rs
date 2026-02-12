use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::langgraph::persistence::error::PersistenceError;

/// Trait for tasks that can be cached and resumed
///
/// Tasks wrap non-deterministic operations and side effects,
/// allowing them to be cached and reused when resuming from checkpoints.
#[async_trait]
pub trait Task: Send + Sync {
    /// Execute the task with given input
    async fn execute(&self, input: Value) -> Result<Value, TaskError>;

    /// Generate a cache key for the task based on input
    ///
    /// This key is used to cache task results and retrieve them
    /// when resuming from checkpoints.
    fn cache_key(&self, input: &Value) -> String;

    /// Get the task identifier (for logging/debugging)
    fn task_id(&self) -> &str;
}

/// Errors that can occur when working with tasks
#[derive(thiserror::Error, Debug)]
pub enum TaskError {
    #[error("Task execution error: {0}")]
    ExecutionError(String),

    #[error("Task cache error: {0}")]
    CacheError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Persistence error: {0}")]
    PersistenceError(#[from] PersistenceError),
}

pub type TaskResult<T> = Result<T, TaskError>;

/// Function-based task implementation
///
/// Wraps an async function to make it a Task.
pub struct FunctionTask<F> {
    task_id: String,
    func: Arc<F>,
}

impl<F> FunctionTask<F>
where
    F: Fn(
            Value,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, TaskError>> + Send>>
        + Send
        + Sync
        + 'static,
{
    /// Create a new function task
    pub fn new(task_id: impl Into<String>, func: F) -> Self {
        Self {
            task_id: task_id.into(),
            func: Arc::new(func),
        }
    }
}

#[async_trait]
impl<F> Task for FunctionTask<F>
where
    F: Fn(
            Value,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, TaskError>> + Send>>
        + Send
        + Sync
        + 'static,
{
    async fn execute(&self, input: Value) -> Result<Value, TaskError> {
        (self.func)(input).await
    }

    fn cache_key(&self, input: &Value) -> String {
        // Generate cache key from task_id and input
        // Use a hash of the serialized input for efficiency
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.task_id.hash(&mut hasher);
        serde_json::to_string(input)
            .unwrap_or_default()
            .hash(&mut hasher);
        format!("task:{}:{}", self.task_id, hasher.finish())
    }

    fn task_id(&self) -> &str {
        &self.task_id
    }
}

/// Type alias for a boxed task
pub type TaskBox = Arc<dyn Task>;
