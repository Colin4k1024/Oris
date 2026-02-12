//! Built-in tools for Deep Agent: write_todos, file system (ls, read_file, write_file, edit_file, glob, grep), task.

mod task;
mod tool_wrapper;
mod write_todos;

pub use task::TaskTool;
pub use tool_wrapper::ToolWithCustomDescription;
pub use write_todos::{TodoItem, TodoStatus, WriteTodosTool};

pub mod fs;
pub use fs::{EditFileTool, GlobTool, GrepTool, LsTool, ReadFileTool, WriteFileTool};
