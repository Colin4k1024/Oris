//! ls tool: list directory entries (name, type, size, modified time).

use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::common::{resolve_in_workspace, workspace_root_from_context, FileSystemToolError};
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Tool that lists files and directories in the workspace.
pub struct LsTool {
    workspace_root: Option<std::path::PathBuf>,
}

impl LsTool {
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

impl Default for LsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> String {
        "ls".to_string()
    }

    fn description(&self) -> String {
        "List files and directories at the given path (relative to workspace root). \
         Returns name, type (file/dir), size, and last modified time."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to workspace root; default '.'"
                }
            }
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "ls requires runtime (workspace). Use run_with_runtime.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        let path_str = input.get("path").and_then(Value::as_str).unwrap_or(".");
        if let Some(backend) = runtime.file_backend() {
            let infos = backend.ls_info(path_str).await.map_err(|e| e.to_string())?;
            let entries: Vec<Value> = infos
                .into_iter()
                .map(|f| {
                    json!({
                        "name": f.path.rsplit('/').next().unwrap_or(&f.path),
                        "type": if f.is_dir { "dir" } else { "file" },
                        "size": f.size,
                        "modified": f.modified_at.unwrap_or(0)
                    })
                })
                .collect();
            return Ok(ToolResult::Text(serde_json::to_string_pretty(&entries)?));
        }
        let root = workspace_root_from_context(self.workspace_root.as_ref(), runtime.context())
            .ok_or(FileSystemToolError::WorkspaceNotSet)?;
        let path = resolve_in_workspace(&root, path_str)?;
        if !path.is_dir() {
            return Err(FileSystemToolError::InvalidPath(format!(
                "Not a directory: {}",
                path.display()
            ))
            .into());
        }
        let mut entries: Vec<Value> = Vec::new();
        for e in std::fs::read_dir(&path)? {
            let e = e?;
            let meta = e.metadata()?;
            let file_type = if meta.is_dir() { "dir" } else { "file" };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs())
                })
                .unwrap_or(0);
            entries.push(json!({
                "name": e.file_name().to_string_lossy(),
                "type": file_type,
                "size": meta.len(),
                "modified": modified
            }));
        }
        entries.sort_by(|a, b| {
            let na = a.get("name").and_then(Value::as_str).unwrap_or("");
            let nb = b.get("name").and_then(Value::as_str).unwrap_or("");
            na.cmp(nb)
        });
        Ok(ToolResult::Text(serde_json::to_string_pretty(&entries)?))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}
