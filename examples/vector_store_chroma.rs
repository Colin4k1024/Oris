// To run: cargo run --example vector_store_chroma --features chroma
// Requires: Chroma running (e.g. docker, or Chroma client default http://localhost:8000).
// OPENAI_API_KEY for embedder.

#[cfg(feature = "chroma")]
use oris::{
    embedding::openai::openai_embedder::OpenAiEmbedder, schemas::Document,
    vectorstore::chroma::StoreBuilder, vectorstore::VecStoreOptions, vectorstore::VectorStore,
};

#[cfg(feature = "chroma")]
#[tokio::main]
async fn main() {
    let embedder = OpenAiEmbedder::default();
    let store = StoreBuilder::new()
        .embedder(embedder)
        .collection_name("oris-example")
        .build()
        .await
        .unwrap();

    let doc1 =
        Document::new("oris is a programmable AI execution runtime in Rust.");
    let doc2 = Document::new("oris is a programmable AI execution runtime in Rust.");
    let doc3 = Document::new("Capital of USA is Washington D.C. Capital of France is Paris.");

    let opt = VecStoreOptions::default();
    let _ids = store
        .add_documents(&[doc1, doc2, doc3], &opt)
        .await
        .unwrap();

    let results = store
        .similarity_search("capital of France", 2, &opt)
        .await
        .unwrap();
    for r in &results {
        println!("  {}", r.page_content);
    }
}

#[cfg(not(feature = "chroma"))]
fn main() {
    println!("Run: cargo run --example vector_store_chroma --features chroma");
}
