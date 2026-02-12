use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::RwLock;

use crate::langgraph::persistence::checkpointer::CheckpointerBox;
use crate::langgraph::state::State;

use super::{Task, TaskError};

/// Task result cache
///
/// Caches task results to avoid re-executing tasks when resuming from checkpoints.
pub struct TaskCache {
    cache: Arc<RwLock<HashMap<String, Value>>>,
}

impl TaskCache {
    /// Create a new task cache
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get a cached task result
    pub async fn get(&self, key: &str) -> Option<Value> {
        let cache = self.cache.read().await;
        cache.get(key).cloned()
    }

    /// Store a task result in cache
    pub async fn put(&self, key: String, value: Value) {
        let mut cache = self.cache.write().await;
        cache.insert(key, value);
    }

    /// Clear the cache
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

impl Default for TaskCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a task with caching support
///
/// This function checks the cache first, and if not found, executes the task
/// and caches the result.
pub async fn execute_task_with_cache(
    task: &dyn Task,
    input: Value,
    cache: Option<&TaskCache>,
) -> Result<Value, TaskError> {
    let cache_key = task.cache_key(&input);

    // Check cache first
    if let Some(cache) = cache {
        if let Some(cached_result) = cache.get(&cache_key).await {
            log::debug!("Task {} cache hit", task.task_id());
            return Ok(cached_result);
        }
    }

    // Execute task
    log::debug!("Task {} cache miss, executing", task.task_id());
    let result = task.execute(input).await?;

    // Store in cache
    if let Some(cache) = cache {
        cache.put(cache_key, result.clone()).await;
    }

    Ok(result)
}

/// Load task cache from checkpointer
///
/// When resuming from a checkpoint, we can load previously cached task results.
pub async fn load_task_cache_from_checkpoint<S: State>(
    checkpointer: &CheckpointerBox<S>,
    thread_id: &str,
) -> Result<TaskCache, TaskError> {
    let cache = TaskCache::new();

    // Try to get the latest checkpoint
    if let Some(snapshot) = checkpointer.get(thread_id, None).await? {
        // Extract task results from checkpoint metadata
        if let Some(task_results) = snapshot.metadata.get("task_results") {
            if let Some(task_map) = task_results.as_object() {
                let mut cache_map = cache.cache.write().await;
                for (key, value) in task_map {
                    cache_map.insert(key.clone(), value.clone());
                }
            }
        }
    }

    Ok(cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_task_cache() {
        let cache = TaskCache::new();

        let key = "test_key".to_string();
        let value = serde_json::json!("test_value");

        cache.put(key.clone(), value.clone()).await;
        let retrieved = cache.get(&key).await;
        assert_eq!(retrieved, Some(value));
    }
}
