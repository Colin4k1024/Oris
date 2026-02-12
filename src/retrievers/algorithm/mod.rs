//! Algorithm-based retrievers
//!
//! These retrievers use classical information retrieval algorithms like BM25, TF-IDF, and SVM.
//! They don't require external services and work with pre-indexed documents.

#[cfg(feature = "bm25")]
mod bm25_retriever;
#[cfg(feature = "bm25")]
pub use bm25_retriever::*;

#[cfg(feature = "tfidf")]
mod tfidf_retriever;
#[cfg(feature = "tfidf")]
pub use tfidf_retriever::*;

#[cfg(feature = "svm")]
mod svm_retriever;
#[cfg(feature = "svm")]
pub use svm_retriever::*;
