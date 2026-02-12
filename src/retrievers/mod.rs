//! Retrievers module
//!
//! This module provides various retriever implementations for document retrieval.
//! All retrievers implement the `Retriever` trait from `crate::schemas::Retriever`.

mod error;
pub use error::*;

mod external;
pub use external::*;

mod algorithm;
pub use algorithm::*;

mod reranker;
pub use reranker::*;

mod hybrid;
pub use hybrid::*;

mod query_enhancement;
pub use query_enhancement::*;

mod compression;
pub use compression::*;
