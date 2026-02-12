//! Store backend: files in ToolStore (cross-thread durable).
//!
//! See [Backends – StoreBackend](https://docs.langchain.com/oss/python/deepagents/backends#storebackend-langgraph-store).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::{EditResult, FileBackend, FileInfo, GrepMatch, ToolStore, WriteResult};

const FS_NAMESPACE: &[&str] = &["fs"];

/// Backend that stores file contents in a ToolStore (e.g. InMemoryStore, Redis).
pub struct StoreBackend {
    store: Arc<dyn ToolStore>,
}

impl StoreBackend {
    pub fn new(store: Arc<dyn ToolStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl FileBackend for StoreBackend {
    async fn ls_info(&self, path: &str) -> Result<Vec<FileInfo>, String> {
        let prefix = path.trim().trim_end_matches('/');
        let prefix_key = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", prefix)
        };
        let all = self.store.list(FS_NAMESPACE).await;
        let mut keys: Vec<String> = all
            .into_iter()
            .filter(|k| prefix_key.is_empty() || k == prefix.trim() || k.starts_with(&prefix_key))
            .collect();
        keys.sort();
        let mut infos = Vec::new();
        for k in keys {
            let value = self.store.get(FS_NAMESPACE, &k).await;
            let content = value
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            let is_dir = false;
            infos.push(FileInfo {
                path: k,
                is_dir,
                size: content.len() as u64,
                modified_at: None,
            });
        }
        Ok(infos)
    }

    async fn read(&self, file_path: &str, offset: u32, limit: u32) -> Result<String, String> {
        let path = file_path.trim().trim_start_matches('/');
        let value = self
            .store
            .get(FS_NAMESPACE, path)
            .await
            .ok_or_else(|| format!("Error: File '/{}' not found", path))?;
        let content = value
            .as_str()
            .ok_or_else(|| format!("Error: File '/{}' not found", path))?;
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
        Ok(selected
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{:6}\t{}", start + i + 1, s))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    async fn write(&self, file_path: &str, content: &str) -> Result<WriteResult, String> {
        let path = file_path.trim().trim_start_matches('/');
        self.store
            .put(FS_NAMESPACE, path, Value::String(content.to_string()))
            .await;
        Ok(WriteResult {
            error: None,
            path: Some(path.to_string()),
        })
    }

    async fn edit(
        &self,
        file_path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<EditResult, String> {
        let path = file_path.trim().trim_start_matches('/');
        let value = self
            .store
            .get(FS_NAMESPACE, path)
            .await
            .ok_or_else(|| format!("Error: File '/{}' not found", path))?;
        let content = value.as_str().unwrap_or("");
        let (new_content, count) = if replace_all {
            let n = content.matches(old_string).count();
            (content.replace(old_string, new_string), n as u32)
        } else {
            if content.matches(old_string).count() != 1 {
                return Err(
                    "Expected exactly one occurrence of old_string (use replace_all for multiple)"
                        .to_string(),
                );
            }
            let mut n = 0u32;
            let out = content.replacen(old_string, new_string, 1);
            if out != content {
                n = 1;
            }
            (out, n)
        };
        self.store
            .put(FS_NAMESPACE, path, Value::String(new_content))
            .await;
        Ok(EditResult {
            error: None,
            path: Some(path.to_string()),
            occurrences: Some(count),
        })
    }

    async fn glob_info(&self, pattern: &str, path: &str) -> Result<Vec<FileInfo>, String> {
        let pat = glob::Pattern::new(pattern).map_err(|e| e.to_string())?;
        let prefix = path.trim().trim_end_matches('/');
        let prefix_key = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", prefix)
        };
        let all = self.store.list(FS_NAMESPACE).await;
        let mut out = Vec::new();
        for k in all {
            if !prefix_key.is_empty() && !k.starts_with(&prefix_key) && k != prefix {
                continue;
            }
            if !pat.matches(&k) {
                continue;
            }
            let value = self.store.get(FS_NAMESPACE, &k).await;
            let content = value
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            out.push(FileInfo {
                path: k,
                is_dir: false,
                size: content.len() as u64,
                modified_at: None,
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
        let all = self.store.list(FS_NAMESPACE).await;
        let filtered: Vec<String> = if let Some(p) = path {
            let prefix = p.trim().trim_end_matches('/');
            let prefix_key = if prefix.is_empty() {
                String::new()
            } else {
                format!("{}/", prefix)
            };
            all.into_iter()
                .filter(|k| prefix_key.is_empty() || k == prefix || k.starts_with(&prefix_key))
                .collect()
        } else {
            all
        };
        let filtered: Vec<String> = if let Some(gp) = glob_pattern {
            let pat = glob::Pattern::new(gp).map_err(|e| e.to_string())?;
            filtered.into_iter().filter(|k| pat.matches(k)).collect()
        } else {
            filtered
        };
        let mut matches = Vec::new();
        for k in filtered {
            let value = match self.store.get(FS_NAMESPACE, &k).await {
                Some(v) => v,
                None => continue,
            };
            let content = match value.as_str() {
                Some(s) => s,
                None => continue,
            };
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    matches.push(GrepMatch {
                        path: k.clone(),
                        line: (line_num + 1) as u32,
                        text: line.trim().to_string(),
                    });
                }
            }
        }
        Ok(matches)
    }
}
