//! Shared path resolution and safety for file system tools.
//!
//! See [harness – File system access](https://docs.langchain.com/oss/python/deepagents/harness#file-system-access).

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileSystemToolError {
    #[error("Path escapes workspace: {0}")]
    PathEscapesWorkspace(String),
    #[error("Workspace root not set; set workspace_root in config or context")]
    WorkspaceNotSet,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

/// Resolve a path relative to workspace root and ensure it stays inside the root.
pub fn resolve_in_workspace(
    workspace_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, FileSystemToolError> {
    let trimmed = relative_path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Ok(workspace_root.to_path_buf());
    }
    if trimmed.contains("..") {
        return Err(FileSystemToolError::PathEscapesWorkspace(
            "Path must not contain '..'".to_string(),
        ));
    }
    let full = workspace_root.join(trimmed);
    let canonical_workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if full.exists() {
        let canonical_full = full.canonicalize()?;
        if !canonical_full.starts_with(&canonical_workspace) {
            return Err(FileSystemToolError::PathEscapesWorkspace(
                canonical_full.display().to_string(),
            ));
        }
        Ok(canonical_full)
    } else {
        if !full.starts_with(workspace_root) && !full.starts_with(&canonical_workspace) {
            return Err(FileSystemToolError::PathEscapesWorkspace(
                full.display().to_string(),
            ));
        }
        Ok(full)
    }
}

/// Get workspace root from optional config path and context.
pub fn workspace_root_from_context(
    config_root: Option<&PathBuf>,
    context: &dyn crate::tools::ToolContext,
) -> Option<PathBuf> {
    if let Some(p) = config_root {
        return Some(p.clone());
    }
    context.get("workspace_root").map(PathBuf::from)
}

/// Recursively collect all file paths under `dir` that stay under `workspace_root`.
/// Returns paths as relative strings (forward slashes) for glob matching.
pub fn list_files_under_workspace(
    workspace_root: &Path,
    dir: &Path,
) -> Result<Vec<String>, FileSystemToolError> {
    let canonical_workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let mut out = Vec::new();
    list_files_under_workspace_impl(&canonical_workspace, dir, &canonical_workspace, &mut out)?;
    Ok(out)
}

fn list_files_under_workspace_impl(
    workspace_root: &Path,
    dir: &Path,
    canonical_workspace: &Path,
    out: &mut Vec<String>,
) -> Result<(), FileSystemToolError> {
    if !dir.is_dir() {
        return Ok(());
    }
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let path = e.path();
        if path.is_dir() {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !canonical.starts_with(canonical_workspace) {
                continue;
            }
            list_files_under_workspace_impl(workspace_root, &path, canonical_workspace, out)?;
        } else {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !canonical.starts_with(canonical_workspace) {
                continue;
            }
            let rel = path
                .strip_prefix(workspace_root)
                .unwrap_or_else(|_| path.as_path());
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
