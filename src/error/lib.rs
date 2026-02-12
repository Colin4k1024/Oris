//! oris 错误处理模块
//!
//! 提供统一的错误类型定义和处理模式。
//! 
//! # 使用示例
//!
//! ```rust
//! use oris::error::LangChainError;
//!
//! async fn example() -> Result<(), LangChainError> {
//!     // 使用 ? 操作符传播错误
//!     some_operation().await?;
//!     Ok(())
//! }
//! ```

pub mod chain;
pub mod llm;
pub mod retriever;
pub mod tool;
pub mod vectorstore;

pub use crate::error::chain::ChainError;
pub use crate::error::llm::LLMError;
pub use crate::error::retriever::RetrieverError;
pub use crate::error::tool::ToolError;
pub use crate::error::vectorstore::VectorStoreError;
pub use LangChainError;