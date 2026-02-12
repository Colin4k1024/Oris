//! Workspace (local disk) backend: paths under a root directory.
//!
//! See [Backends – FilesystemBackend](https://docs.langchain.com/oss/python/deepagents/backends#filesystembackend-local-disk).

use std::path::PathBuf;

use async_trait::async_trait;

use crate::agent::deep_agent::tools::fs::common::{
    list_files_under_workspace, resolve_in_workspace,
};
use crate::tools::{EditResult, FileBackend, FileInfo, GrepMatch, WriteResult};

/// Backend that reads/writes real files under a workspace root (sandboxed).
pub struct WorkspaceBackend {
    workspace_root: PathBuf,
}

impl WorkspaceBackend {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

#[async_trait]
impl FileBackend for WorkspaceBackend {
    async fn ls_info(&self, path: &str) -> Result<Vec<FileInfo>, String> {
        let path_buf =
            resolve_in_workspace(&self.workspace_root, path).map_err(|e| e.to_string())?;
        if !path_buf.is_dir() {
            return Err(format!("Not a directory: {}", path));
        }
        let mut out = Vec::new();
        for e in std::fs::read_dir(&path_buf).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let path = e.path();
            let meta = e.metadata().map_err(|e| e.to_string())?;
            let is_dir = meta.is_dir();
            let modified_at = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let rel = path
                .strip_prefix(&self.workspace_root)
                .unwrap_or_else(|_| path.as_path());
            out.push(FileInfo {
                path: rel.to_string_lossy().replace('\\', "/"),
                is_dir,
                size: meta.len(),
                modified_at,
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    async fn read(&self, file_path: &str, offset: u32, limit: u32) -> Result<String, String> {
        let path =
            resolve_in_workspace(&self.workspace_root, file_path).map_err(|e| e.to_string())?;
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let lines: Vec<&str> = content.lines().collect();
        let start = if offset > 0 {
            ((offset as usize).saturating_sub(1)).min(lines.len())
        } else {
            0
        };
        let end = if limit > 0 {
            (start + limit as usize).min(lines.len())
        } else {
            lines.len()
        };
        let selected = &lines[start..end];
        let out: String = selected
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{:6}\t{}", start + i + 1, s))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(out)
    }

    async fn write(&self, file_path: &str, content: &str) -> Result<WriteResult, String> {
        let path =
            resolve_in_workspace(&self.workspace_root, file_path).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        let rel = path
            .strip_prefix(&self.workspace_root)
            .unwrap_or_else(|_| path.as_path());
        Ok(WriteResult {
            error: None,
            path: Some(rel.to_string_lossy().replace('\\', "/")),
        })
    }

    async fn edit(
        &self,
        file_path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<EditResult, String> {
        let path =
            resolve_in_workspace(&self.workspace_root, file_path).map_err(|e| e.to_string())?;
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let (new_content, occurrences) = if replace_all {
            let count = content.matches(old_string).count();
            (content.replace(old_string, new_string), count as u32)
        } else {
            if content.matches(old_string).count() != 1 {
                return Err(format!(
                    "Expected exactly one occurrence of old_string (use replace_all for multiple)"
                ));
            }
            let mut n = 0u32;
            let out = content.replacen(old_string, new_string, 1);
            if out != content {
                n = 1;
            }
            (out, n)
        };
        std::fs::write(&path, &new_content).map_err(|e| e.to_string())?;
        let rel = path
            .strip_prefix(&self.workspace_root)
            .unwrap_or_else(|_| path.as_path());
        Ok(EditResult {
            error: None,
            path: Some(rel.to_string_lossy().replace('\\', "/")),
            occurrences: Some(occurrences),
        })
    }

    async fn glob_info(&self, pattern: &str, path: &str) -> Result<Vec<FileInfo>, String> {
        let base = resolve_in_workspace(&self.workspace_root, path).map_err(|e| e.to_string())?;
        if !base.is_dir() {
            return Err(format!("Not a directory: {}", path));
        }
        let pat = glob::Pattern::new(pattern).map_err(|e| e.to_string())?;
        let all =
            list_files_under_workspace(&self.workspace_root, &base).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for rel in all {
            if !pat.matches(&rel) {
                continue;
            }
            let full = self.workspace_root.join(&rel);
            let meta = std::fs::metadata(&full).ok();
            out.push(FileInfo {
                path: rel,
                is_dir: meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified_at: meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    async fn grep_raw(
        &self,
        pattern: &str,
        path: Option<&str>,
        glob_pattern: Option<&str>,
    ) -> Result<Vec<GrepMatch>, String> {
        let path_str = path.unwrap_or(".");
        let base =
            resolve_in_workspace(&self.workspace_root, path_str).map_err(|e| e.to_string())?;
        let candidates: Vec<String> = if base.is_file() {
            let rel = base
                .strip_prefix(&self.workspace_root)
                .unwrap_or_else(|_| base.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            vec![rel]
        } else {
            let all = list_files_under_workspace(&self.workspace_root, &base)
                .map_err(|e| e.to_string())?;
            if let Some(gp) = glob_pattern {
                let pat = glob::Pattern::new(gp).map_err(|e| e.to_string())?;
                all.into_iter().filter(|p| pat.matches(p)).collect()
            } else {
                all
            }
        };
        let mut matches = Vec::new();
        for rel in candidates {
            let full = self.workspace_root.join(&rel);
            if !full.is_file() {
                continue;
            }
            let content = match std::fs::read_to_string(&full) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    matches.push(GrepMatch {
                        path: rel.clone(),
                        line: (line_num + 1) as u32,
                        text: line.trim().to_string(),
                    });
                }
            }
        }
        Ok(matches)
    }
}
