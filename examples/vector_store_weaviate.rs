// To run: cargo run --example vector_store_weaviate --features weaviate
// Requires: Weaviate at e.g. http://localhost:8080. Create a class with:
//   content (text), metadata (text), vectorizer "none".
// OPENAI_API_KEY for embedder.

#[cfg(feature = "weaviate")]
use oris::{
    embedding::openai::openai_embedder::OpenAiEmbedder, schemas::Document,
    vectorstore::weaviate::StoreBuilder, vectorstore::VecStoreOptions, vectorstore::VectorStore,
};

#[cfg(feature = "weaviate")]
#[tokio::main]
async fn main() {
    let base = std::env::var("WEAVIATE_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let store = StoreBuilder::new()
        .base_url(base)
        .class_name("LangChainDoc")
        .embedder(OpenAiEmbedder::default())
        .build()
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

#[cfg(not(feature = "weaviate"))]
fn main() {
    println!("Run: cargo run --example vector_store_weaviate --features weaviate");
}
