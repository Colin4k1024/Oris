//! read_file tool: read file contents with optional offset/limit.

use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::common::{resolve_in_workspace, workspace_root_from_context, FileSystemToolError};
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Tool that reads file contents, with optional offset and limit for large files.
pub struct ReadFileTool {
    workspace_root: Option<std::path::PathBuf>,
}

impl ReadFileTool {
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

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> String {
        "read_file".to_string()
    }

    fn description(&self) -> String {
        "Read contents of a file. Path is relative to workspace root. \
         Optionally use offset (line number 1-based) and limit (number of lines) for large files."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to workspace" },
                "offset": { "type": "integer", "description": "Start line (1-based); omit to read from start" },
                "limit": { "type": "integer", "description": "Max lines to return; omit for full file" }
            },
            "required": ["path"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "read_file requires runtime (workspace). Use run_with_runtime.".to_string(),
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
        let offset = input.get("offset").and_then(Value::as_u64).unwrap_or(0) as u32;
        let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(0) as u32;
        if let Some(backend) = runtime.file_backend() {
            let content = backend
                .read(path_str, offset, limit)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(ToolResult::Text(content));
        }
        let root = workspace_root_from_context(self.workspace_root.as_ref(), runtime.context())
            .ok_or(FileSystemToolError::WorkspaceNotSet)?;
        let path = resolve_in_workspace(&root, path_str)?;
        let content = std::fs::read_to_string(&path)?;
        let offset_u = offset as usize;
        let limit_u = limit as usize;

        let with_lines: Vec<String> = content.lines().map(String::from).collect();
        let start = if offset_u > 0 {
            (offset_u - 1).min(with_lines.len())
        } else {
            0
        };
        let end = if limit_u > 0 {
            (start + limit_u).min(with_lines.len())
        } else {
            with_lines.len()
        };
        let selected = &with_lines[start..end];
        let with_numbers: String = selected
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{:6}\t{}", start + i + 1, s))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolResult::Text(with_numbers))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}
