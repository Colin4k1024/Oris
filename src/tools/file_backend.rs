//! Pluggable file backend protocol for deep agent filesystem tools.
//!
//! See [Backends](https://docs.langchain.com/oss/python/deepagents/backends).

use async_trait::async_trait;

/// Metadata for a single file or directory.
#[derive(Clone, Debug)]
pub struct FileInfo {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_at: Option<u64>,
}

/// Result of a write operation.
#[derive(Clone, Debug)]
pub struct WriteResult {
    pub error: Option<String>,
    pub path: Option<String>,
}

/// Result of an edit operation.
#[derive(Clone, Debug)]
pub struct EditResult {
    pub error: Option<String>,
    pub path: Option<String>,
    pub occurrences: Option<u32>,
}

/// A single grep match (path, line number, line text).
#[derive(Clone, Debug)]
pub struct GrepMatch {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// Pluggable backend for filesystem tools (ls, read_file, write_file, edit_file, glob, grep).
///
/// Implementations: WorkspaceBackend (disk), StoreBackend (ToolStore), CompositeBackend (routing).
#[async_trait]
pub trait FileBackend: Send + Sync {
    /// List directory entries at the given path. Path is backend-defined (e.g. relative to workspace or virtual path).
    async fn ls_info(&self, path: &str) -> Result<Vec<FileInfo>, String>;

    /// Read file content with optional offset (1-based line) and limit (max lines). Returns numbered lines.
    async fn read(&self, file_path: &str, offset: u32, limit: u32) -> Result<String, String>;

    /// Write content to a file. Create-only or overwrite per backend semantics.
    async fn write(&self, file_path: &str, content: &str) -> Result<WriteResult, String>;

    /// Edit file: replace old_string with new_string. replace_all: replace all occurrences.
    async fn edit(
        &self,
        file_path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<EditResult, String>;

    /// List files matching a glob pattern under the given path.
    async fn glob_info(&self, pattern: &str, path: &str) -> Result<Vec<FileInfo>, String>;

    /// Search file contents. path: restrict to path; glob_pattern: restrict files by glob.
    async fn grep_raw(
        &self,
        pattern: &str,
        path: Option<&str>,
        glob_pattern: Option<&str>,
    ) -> Result<Vec<GrepMatch>, String>;
}
