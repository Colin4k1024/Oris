//! grep tool: search file contents with optional mode (files, content, count).

use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::common::{
    list_files_under_workspace, resolve_in_workspace, workspace_root_from_context,
    FileSystemToolError,
};
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Output mode for grep: filenames only, lines with context, or counts per file.
#[derive(Clone, Copy)]
pub enum GrepMode {
    /// Only list file paths that contain a match.
    Files,
    /// Emit matching lines (and optional context).
    Content,
    /// Emit match count per file.
    Count,
}

/// Tool that searches file contents within the workspace.
pub struct GrepTool {
    workspace_root: Option<std::path::PathBuf>,
}

impl GrepTool {
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

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

fn mode_from_str(s: &str) -> GrepMode {
    match s.to_lowercase().as_str() {
        "files" | "file" => GrepMode::Files,
        "count" | "counts" => GrepMode::Count,
        _ => GrepMode::Content,
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> String {
        "grep".to_string()
    }

    fn description(&self) -> String {
        "Search file contents. Optional mode: 'files' (only filenames), 'content' (matching lines), 'count' (counts per file)."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search string (plain text)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file path relative to workspace; default '.'"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to restrict files to search (e.g. '**/*.rs')"
                },
                "mode": {
                    "type": "string",
                    "description": "Output: 'files', 'content', or 'count'",
                    "enum": ["files", "content", "count"]
                }
            },
            "required": ["query"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "grep requires runtime (workspace). Use run_with_runtime.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| FileSystemToolError::InvalidPath("missing query".to_string()))?;
        let path_str = input.get("path").and_then(Value::as_str);
        let file_pattern = input.get("pattern").and_then(Value::as_str);
        let mode = input
            .get("mode")
            .and_then(Value::as_str)
            .map(mode_from_str)
            .unwrap_or(GrepMode::Content);

        if let Some(backend) = runtime.file_backend() {
            let matches = backend
                .grep_raw(query, path_str.map(|s| s as &str), file_pattern)
                .await
                .map_err(|e| e.to_string())?;
            let files: std::collections::HashSet<String> =
                matches.iter().map(|m| m.path.clone()).collect();
            let files_with_matches: Vec<String> = files.into_iter().collect();
            let mut count_per_file: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for m in &matches {
                *count_per_file.entry(m.path.clone()).or_insert(0) += 1;
            }
            let content_lines: Vec<String> = matches
                .iter()
                .map(|m| format!("{}:{}: {}", m.path, m.line, m.text))
                .collect();
            let output = match mode {
                GrepMode::Files => serde_json::to_string_pretty(&files_with_matches)?,
                GrepMode::Count => {
                    let arr: Vec<Value> = count_per_file
                        .into_iter()
                        .map(|(path, n)| json!({ "path": path, "count": n }))
                        .collect();
                    serde_json::to_string_pretty(&arr)?
                }
                GrepMode::Content => content_lines.join("\n"),
            };
            return Ok(ToolResult::Text(output));
        }

        let root = workspace_root_from_context(self.workspace_root.as_ref(), runtime.context())
            .ok_or(FileSystemToolError::WorkspaceNotSet)?;
        let path_str = path_str.unwrap_or(".");
        let base = resolve_in_workspace(&root, path_str)?;
        let candidate_paths: Vec<String> = if base.is_file() {
            let rel = base
                .strip_prefix(&root)
                .unwrap_or_else(|_| base.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            vec![rel]
        } else {
            let all = list_files_under_workspace(&root, &base)?;
            if let Some(pat) = file_pattern {
                let glob_pat = glob::Pattern::new(pat)
                    .map_err(|e| FileSystemToolError::InvalidPath(e.to_string()))?;
                all.into_iter().filter(|p| glob_pat.matches(p)).collect()
            } else {
                all
            }
        };

        let mut files_with_matches = Vec::new();
        let mut content_lines: Vec<String> = Vec::new();
        let mut count_per_file: Vec<(String, usize)> = Vec::new();

        for rel in candidate_paths {
            let full = root.join(&rel);
            if !full.is_file() {
                continue;
            }
            let content = match std::fs::read_to_string(&full) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let mut count = 0usize;
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(query) {
                    count += 1;
                    if matches!(mode, GrepMode::Content) {
                        content_lines.push(format!("{}:{}: {}", rel, line_num + 1, line.trim()));
                    }
                }
            }
            if count > 0 {
                files_with_matches.push(rel.clone());
                count_per_file.push((rel, count));
            }
        }

        let output = match mode {
            GrepMode::Files => serde_json::to_string_pretty(&files_with_matches)?,
            GrepMode::Count => {
                let arr: Vec<Value> = count_per_file
                    .into_iter()
                    .map(|(path, n)| json!({ "path": path, "count": n }))
                    .collect();
                serde_json::to_string_pretty(&arr)?
            }
            GrepMode::Content => content_lines.join("\n"),
        };
        Ok(ToolResult::Text(output))
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}
