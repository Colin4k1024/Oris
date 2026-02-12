//! write_todos tool: persist and update todo lists in the store (planning).

use std::error::Error;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Todo item for task planning.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TodoItem {
    /// Unique id (optional; can be generated if missing).
    pub id: Option<String>,
    /// Human-readable title.
    pub title: String,
    /// Current status.
    pub status: TodoStatus,
}

/// Status of a todo item.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    Done,
    Cancelled,
}

impl Default for TodoStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Store namespace for todos: ["todos"]; key is thread_id or session_id from context.
pub const TODOS_NAMESPACE: &str = "todos";
pub const TODOS_KEY: &str = "list";

/// Tool that writes a todo list to the store for the current thread/session.
///
/// Uses ToolStore namespace `["todos"]` and key from context (session_id or thread_id),
/// or "default" if neither is set. Merge/replace semantics: pass full list to persist.
pub struct WriteTodosTool;

impl WriteTodosTool {
    pub fn new() -> Self {
        Self
    }

    fn store_key(context: &dyn crate::tools::ToolContext) -> String {
        context
            .session_id()
            .or_else(|| context.get("thread_id"))
            .or_else(|| context.user_id())
            .unwrap_or("default")
            .to_string()
    }
}

impl Default for WriteTodosTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WriteTodosTool {
    fn name(&self) -> String {
        "write_todos".to_string()
    }

    fn description(&self) -> String {
        "Write or update the current to-do list. Use this to break down complex tasks into steps, \
         track progress, and adapt the plan as new information arrives. Pass a JSON array of \
         items with optional 'id', 'title', and 'status' (pending, done, cancelled)."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "Array of todo items. Each may have id (optional), title, status (optional: pending, done, cancelled).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "title": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "done", "cancelled"] }
                        }
                    }
                }
            },
            "required": ["todos"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "write_todos requires runtime (store). Use run_with_runtime.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        let todos_value = input
            .get("todos")
            .or_else(|| input.get("input"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut items: Vec<TodoItem> = Vec::with_capacity(todos_value.len());
        for (i, v) in todos_value.iter().enumerate() {
            let id = v.get("id").and_then(Value::as_str).map(String::from);
            let title = v
                .get("title")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| format!("Item {}", i + 1));
            let status = v
                .get("status")
                .and_then(Value::as_str)
                .and_then(|s| match s.to_lowercase().as_str() {
                    "done" => Some(TodoStatus::Done),
                    "cancelled" => Some(TodoStatus::Cancelled),
                    _ => Some(TodoStatus::Pending),
                })
                .unwrap_or_default();
            items.push(TodoItem {
                id: id.or_else(|| Some(format!("todo_{}", i))),
                title,
                status,
            });
        }

        let key = Self::store_key(runtime.context());
        let namespace: &[&str] = &[TODOS_NAMESPACE];
        let value = serde_json::to_value(&items).map_err(|e| e.to_string())?;
        runtime.store().put(namespace, &key, value).await;

        Ok(ToolResult::Text(format!(
            "Todo list updated ({} items saved for this session).",
            items.len()
        )))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{EmptyContext, InMemoryStore};
    use std::sync::Arc;

    #[test]
    fn test_todo_status_default() {
        assert_eq!(TodoStatus::default(), TodoStatus::Pending);
    }

    #[tokio::test]
    async fn test_write_todos_requires_runtime() {
        let tool = WriteTodosTool::new();
        assert!(tool.requires_runtime());
        let err = tool.run(Value::Null).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_write_todos_run_with_runtime() {
        let tool = WriteTodosTool::new();
        let store = Arc::new(InMemoryStore::new());
        let ctx = Arc::new(EmptyContext);
        let state = Arc::new(tokio::sync::Mutex::new(crate::agent::AgentState::new()));
        let runtime = ToolRuntime::new(state, ctx, store, "call_1".to_string());

        let input = json!({
            "todos": [
                { "title": "First task", "status": "pending" },
                { "title": "Second", "status": "done" }
            ]
        });
        let result = tool.run_with_runtime(input, &runtime).await.unwrap();
        let text = result.into_string();
        assert!(text.contains("2 items"));

        let list = runtime.store().get(&["todos"], "default").await.unwrap();
        let arr = list.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }
}
