//! write_file tool: create or overwrite a file.

use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::common::{resolve_in_workspace, workspace_root_from_context, FileSystemToolError};
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Tool that writes content to a file (creates or overwrites).
pub struct WriteFileTool {
    workspace_root: Option<std::path::PathBuf>,
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self {
            workspace_root: None,
        }
    }

    pub fn with_workspace_root(mut self, root: std::path::PathBuf) -> Self {
        self.workspace_root = Some(root);
        self
    }

    pub fn maybe_workspace_root(mut self, root: Option<std::path::PathBuf>) -> Self {
        self.workspace_root = root;
        self
    }
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> String {
        "write_file".to_string()
    }

    fn description(&self) -> String {
        "Create or overwrite a file. Path is relative to workspace root. Creates parent directories if needed."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to workspace" },
                "content": { "type": "string", "description": "Content to write" }
            },
            "required": ["path", "content"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "write_file requires runtime (workspace). Use run_with_runtime.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        let path_str = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| FileSystemToolError::InvalidPath("missing path".to_string()))?;
        let content = input.get("content").and_then(Value::as_str).unwrap_or("");
        if let Some(backend) = runtime.file_backend() {
            let res = backend
                .write(path_str, content)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(err) = res.error {
                return Err(err.into());
            }
            let p = res.path.as_deref().unwrap_or(path_str);
            return Ok(ToolResult::Text(format!(
                "Wrote {} bytes to {}",
                content.len(),
                p
            )));
        }
        let root = workspace_root_from_context(self.workspace_root.as_ref(), runtime.context())
            .ok_or(FileSystemToolError::WorkspaceNotSet)?;
        let path = resolve_in_workspace(&root, path_str)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        Ok(ToolResult::Text(format!(
            "Wrote {} bytes to {}",
            content.len(),
            path.display()
        )))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}
