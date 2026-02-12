mod error;
mod options;

#[cfg(feature = "postgres")]
pub mod pgvector;

#[cfg(feature = "sqlite-vss")]
pub mod sqlite_vss;

#[cfg(feature = "sqlite-vec")]
pub mod sqlite_vec;

#[cfg(feature = "surrealdb")]
pub mod surrealdb;

#[cfg(feature = "opensearch")]
pub mod opensearch;

#[cfg(feature = "qdrant")]
pub mod qdrant;

#[cfg(feature = "in-memory")]
pub mod in_memory;

#[cfg(feature = "chroma")]
pub mod chroma;

#[cfg(feature = "faiss")]
pub mod faiss;

#[cfg(feature = "mongodb")]
pub mod mongodb;

#[cfg(feature = "pinecone")]
pub mod pinecone;

#[cfg(feature = "weaviate")]
pub mod weaviate;

mod base;
mod vectorstore;

pub use base::{
    VectorStoreBaseConfig, VectorStoreBatch, VectorStoreHelpers, VectorStoreInitializable,
};
pub use error::*;
pub use options::*;
pub use vectorstore::*;
