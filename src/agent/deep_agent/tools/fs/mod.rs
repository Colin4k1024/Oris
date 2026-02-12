//! File system tools for Deep Agent (workspace-scoped).
//!
//! Six tools: ls, read_file, write_file, edit_file, glob, grep.
//! See [harness – File system access](https://docs.langchain.com/oss/python/deepagents/harness#file-system-access).

pub mod common;
pub use common::{
    list_files_under_workspace, resolve_in_workspace, workspace_root_from_context,
    FileSystemToolError,
};

mod edit_file;
mod glob;
mod grep;
mod ls;
mod read_file;
mod write_file;

pub use edit_file::EditFileTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use ls::LsTool;
pub use read_file::ReadFileTool;
pub use write_file::WriteFileTool;
