//! edit_file tool: exact string replacement in a file.

use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::common::{resolve_in_workspace, workspace_root_from_context, FileSystemToolError};
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Tool that performs exact old_string -> new_string replacements in a file.
pub struct EditFileTool {
    workspace_root: Option<std::path::PathBuf>,
}

impl EditFileTool {
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

impl Default for EditFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> String {
        "edit_file".to_string()
    }

    fn description(&self) -> String {
        "Edit a file by exact string replacement. Pass one or more replacements (old_string, new_string). \
         Path is relative to workspace root."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to workspace" },
                "replacements": {
                    "type": "array",
                    "description": "List of { old_string, new_string } replacements",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string" },
                            "new_string": { "type": "string" }
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path", "replacements"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "edit_file requires runtime (workspace). Use run_with_runtime.".to_string(),
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
        let replacements = input
            .get("replacements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Some(backend) = runtime.file_backend() {
            let mut total = 0u32;
            for r in replacements {
                let old_s = r.get("old_string").and_then(Value::as_str).unwrap_or("");
                let new_s = r.get("new_string").and_then(Value::as_str).unwrap_or("");
                if old_s.is_empty() {
                    continue;
                }
                let res = backend
                    .edit(path_str, old_s, new_s, false)
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(err) = res.error {
                    return Err(err.into());
                }
                total += res.occurrences.unwrap_or(0);
            }
            return Ok(ToolResult::Text(format!(
                "Applied {} replacement(s) to {}",
                total, path_str
            )));
        }
        let root = workspace_root_from_context(self.workspace_root.as_ref(), runtime.context())
            .ok_or(FileSystemToolError::WorkspaceNotSet)?;
        let path = resolve_in_workspace(&root, path_str)?;
        let content = std::fs::read_to_string(&path)?;
        let mut out = content;
        let mut count = 0;
        for r in replacements {
            let old_s = r.get("old_string").and_then(Value::as_str).unwrap_or("");
            let new_s = r.get("new_string").and_then(Value::as_str).unwrap_or("");
            if !old_s.is_empty() && out.contains(old_s) {
                out = out.replace(old_s, new_s);
                count += 1;
            }
        }
        std::fs::write(&path, &out)?;
        Ok(ToolResult::Text(format!(
            "Applied {} replacement(s) to {}",
            count,
            path.display()
        )))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}
