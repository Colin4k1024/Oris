//! glob tool: find files matching a pattern (e.g. `**/*.rs`, `*.toml`).

use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::common::{
    list_files_under_workspace, resolve_in_workspace, workspace_root_from_context,
    FileSystemToolError,
};
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Tool that finds files matching a glob pattern within the workspace.
pub struct GlobTool {
    workspace_root: Option<std::path::PathBuf>,
}

impl GlobTool {
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

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> String {
        "glob".to_string()
    }

    fn description(&self) -> String {
        "Find files matching a glob pattern (e.g. **/*.rs, *.toml). \
         Path is relative to workspace root; optionally restrict to a subdirectory."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g. **/*.rs, *.toml)"
                },
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search; default '.'"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "glob requires runtime (workspace). Use run_with_runtime.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        let pattern_str = input
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| FileSystemToolError::InvalidPath("missing pattern".to_string()))?;
        let path_str = input.get("path").and_then(Value::as_str).unwrap_or(".");
        if let Some(backend) = runtime.file_backend() {
            let infos = backend
                .glob_info(pattern_str, path_str)
                .await
                .map_err(|e| e.to_string())?;
            let matching: Vec<Value> = infos
                .into_iter()
                .map(|f| {
                    json!({
                        "path": f.path,
                        "size": f.size,
                        "modified": f.modified_at.unwrap_or(0)
                    })
                })
                .collect();
            return Ok(ToolResult::Text(serde_json::to_string_pretty(&matching)?));
        }
        let root = workspace_root_from_context(self.workspace_root.as_ref(), runtime.context())
            .ok_or(FileSystemToolError::WorkspaceNotSet)?;
        let base_dir = resolve_in_workspace(&root, path_str)?;
        if !base_dir.is_dir() {
            return Err(FileSystemToolError::InvalidPath(format!(
                "Not a directory: {}",
                base_dir.display()
            ))
            .into());
        }

        let pattern = glob::Pattern::new(pattern_str)
            .map_err(|e| FileSystemToolError::InvalidPath(e.to_string()))?;

        let all_rel = list_files_under_workspace(&root, &base_dir)?;
        let mut matching: Vec<Value> = Vec::new();
        for rel in all_rel {
            if !pattern.matches(&rel) {
                continue;
            }
            let full = root.join(&rel);
            let meta = std::fs::metadata(&full).ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            matching.push(json!({
                "path": rel,
                "size": size,
                "modified": modified
            }));
        }
        matching.sort_by(|a, b| {
            let pa = a.get("path").and_then(Value::as_str).unwrap_or("");
            let pb = b.get("path").and_then(Value::as_str).unwrap_or("");
            pa.cmp(pb)
        });
        Ok(ToolResult::Text(serde_json::to_string_pretty(&matching)?))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}
