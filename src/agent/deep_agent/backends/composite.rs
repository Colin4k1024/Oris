//! Composite backend: route by path prefix (longest-prefix match).
//!
//! See [Backends – CompositeBackend](https://docs.langchain.com/oss/python/deepagents/backends#compositebackend-default--prefix-routes).

use std::sync::Arc;

use async_trait::async_trait;

use crate::tools::{EditResult, FileBackend, FileInfo, GrepMatch, WriteResult};

/// Route entry: path prefix (e.g. `"store"` or `"memory/"`) and the backend for that prefix.
/// Longest matching prefix wins.
fn choose_backend<'a>(
    path: &str,
    default: &'a Arc<dyn FileBackend>,
    routes: &'a [(String, Arc<dyn FileBackend>)],
) -> (&'a Arc<dyn FileBackend>, String) {
    let path_trim = path.trim().trim_start_matches('/');
    let mut best_len = 0usize;
    let mut chosen = default;
    for (prefix, backend) in routes {
        let p = prefix.trim().trim_end_matches('/');
        if p.is_empty() {
            continue;
        }
        let match_len = if path_trim == p {
            p.len()
        } else if path_trim.starts_with(p) {
            let rest = path_trim.get(p.len()..).unwrap_or("");
            if rest.is_empty() || rest.starts_with('/') {
                p.len()
            } else {
                0
            }
        } else {
            0
        };
        if match_len > best_len {
            best_len = match_len;
            chosen = backend;
        }
    }
    let inner_path = if best_len == 0 {
        path_trim.to_string()
    } else {
        path_trim[best_len..].trim_start_matches('/').to_string()
    };
    (chosen, inner_path)
}

fn restore_prefix(prefix: &str, inner_path: &str) -> String {
    let p = prefix.trim_end_matches('/');
    if inner_path.is_empty() {
        p.to_string()
    } else if p.is_empty() {
        inner_path.to_string()
    } else {
        format!("{}/{}", p, inner_path)
    }
}

/// Composite backend: default backend plus prefix-routed backends.
/// Longest matching prefix selects the backend; path is stripped before delegating
/// and the prefix is restored in results.
pub struct CompositeBackend {
    default: Arc<dyn FileBackend>,
    routes: Vec<(String, Arc<dyn FileBackend>)>,
}

impl CompositeBackend {
    /// Build a composite with a default backend and no routes.
    pub fn new(default: Arc<dyn FileBackend>) -> Self {
        Self {
            default,
            routes: Vec::new(),
        }
    }

    /// Add a route: requests whose path has this prefix use the given backend.
    /// Routes are matched by longest prefix. Prefix is stripped when calling the backend.
    pub fn with_route(mut self, prefix: impl Into<String>, backend: Arc<dyn FileBackend>) -> Self {
        self.routes.push((prefix.into(), backend));
        self.routes.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        self
    }
}

#[async_trait]
impl FileBackend for CompositeBackend {
    async fn ls_info(&self, path: &str) -> Result<Vec<FileInfo>, String> {
        let (backend, inner_path) = choose_backend(path, &self.default, &self.routes);
        let prefix = path.trim().trim_start_matches('/');
        let prefix_len = prefix.len().saturating_sub(inner_path.len());
        let prefix_str = if prefix_len > 0 && prefix_len <= prefix.len() {
            prefix[..prefix_len].trim_end_matches('/')
        } else {
            ""
        };
        let infos = backend
            .ls_info(if inner_path.is_empty() {
                "."
            } else {
                &inner_path
            })
            .await?;
        let out: Vec<FileInfo> = infos
            .into_iter()
            .map(|f| FileInfo {
                path: restore_prefix(prefix_str, &f.path),
                is_dir: f.is_dir,
                size: f.size,
                modified_at: f.modified_at,
            })
            .collect();
        Ok(out)
    }

    async fn read(&self, file_path: &str, offset: u32, limit: u32) -> Result<String, String> {
        let (backend, inner_path) = choose_backend(file_path, &self.default, &self.routes);
        backend.read(&inner_path, offset, limit).await
    }

    async fn write(&self, file_path: &str, content: &str) -> Result<WriteResult, String> {
        let (backend, inner_path) = choose_backend(file_path, &self.default, &self.routes);
        let res = backend.write(&inner_path, content).await?;
        let full = file_path.trim().trim_start_matches('/');
        let prefix_str = if inner_path.is_empty() {
            ""
        } else {
            let n = full.len().saturating_sub(inner_path.len());
            if n > 0 && n <= full.len() {
                full[..n].trim_end_matches('/')
            } else {
                ""
            }
        };
        let path = res.path.as_ref().map(|p| restore_prefix(prefix_str, p));
        Ok(WriteResult {
            error: res.error,
            path: path.or(res.path),
        })
    }

    async fn edit(
        &self,
        file_path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<EditResult, String> {
        let (backend, inner_path) = choose_backend(file_path, &self.default, &self.routes);
        let res = backend
            .edit(&inner_path, old_string, new_string, replace_all)
            .await?;
        let full = file_path.trim().trim_start_matches('/');
        let prefix_str = if inner_path.is_empty() {
            ""
        } else {
            let n = full.len().saturating_sub(inner_path.len());
            if n > 0 && n <= full.len() {
                full[..n].trim_end_matches('/')
            } else {
                ""
            }
        };
        let path = res.path.as_ref().map(|p| restore_prefix(prefix_str, p));
        Ok(EditResult {
            error: res.error,
            path: path.or(res.path),
            occurrences: res.occurrences,
        })
    }

    async fn glob_info(&self, pattern: &str, path: &str) -> Result<Vec<FileInfo>, String> {
        let (backend, inner_path) = choose_backend(path, &self.default, &self.routes);
        let full = path.trim().trim_start_matches('/');
        let prefix_len = full.len().saturating_sub(inner_path.len());
        let prefix_str = if prefix_len > 0 && prefix_len <= full.len() {
            full[..prefix_len].trim_end_matches('/')
        } else {
            ""
        };
        let infos = backend
            .glob_info(
                pattern,
                if inner_path.is_empty() {
                    "."
                } else {
                    &inner_path
                },
            )
            .await?;
        let out: Vec<FileInfo> = infos
            .into_iter()
            .map(|f| FileInfo {
                path: restore_prefix(prefix_str, &f.path),
                is_dir: f.is_dir,
                size: f.size,
                modified_at: f.modified_at,
            })
            .collect();
        Ok(out)
    }

    async fn grep_raw(
        &self,
        pattern: &str,
        path: Option<&str>,
        glob_pattern: Option<&str>,
    ) -> Result<Vec<GrepMatch>, String> {
        match path {
            Some(p) => {
                let (backend, inner) = choose_backend(p, &self.default, &self.routes);
                let full = p.trim().trim_start_matches('/');
                let prefix_len = full.len().saturating_sub(inner.len());
                let prefix_str = if prefix_len > 0 && prefix_len <= full.len() {
                    full[..prefix_len].trim_end_matches('/')
                } else {
                    ""
                };
                let matches = backend
                    .grep_raw(
                        pattern,
                        Some(if inner.is_empty() { "." } else { &inner }),
                        glob_pattern,
                    )
                    .await?;
                let out: Vec<GrepMatch> = matches
                    .into_iter()
                    .map(|m| GrepMatch {
                        path: restore_prefix(prefix_str, &m.path),
                        line: m.line,
                        text: m.text,
                    })
                    .collect();
                Ok(out)
            }
            None => self.default.grep_raw(pattern, None, glob_pattern).await,
        }
    }
}
