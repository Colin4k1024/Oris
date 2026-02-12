// To run: cargo run --example vector_store_in_memory --features in-memory
// No external services required. Uses OpenAiEmbedder (OPENAI_API_KEY).

#[cfg(feature = "in-memory")]
use oris::{
    embedding::openai::openai_embedder::OpenAiEmbedder, schemas::Document,
    vectorstore::in_memory::StoreBuilder, vectorstore::VecStoreOptions, vectorstore::VectorStore,
};

#[cfg(feature = "in-memory")]
#[tokio::main]
async fn main() {
    let embedder = OpenAiEmbedder::default();
    let store = StoreBuilder::new().embedder(embedder).build().unwrap();

    let doc1 =
        Document::new("oris is a programmable AI execution runtime in Rust.");
    let doc2 = Document::new("oris is a programmable AI execution runtime in Rust.");
    let doc3 = Document::new("Capital of USA is Washington D.C. Capital of France is Paris.");

    let opt = VecStoreOptions::default();
    let ids = store
        .add_documents(&[doc1, doc2, doc3], &opt)
        .await
        .unwrap();
    println!("Added {} documents", ids.len());

    let results = store
        .similarity_search("capital of France", 2, &opt)
        .await
        .unwrap();
    for r in &results {
        println!("  {}", r.page_content);
    }

    // In-memory supports delete
    let _ = store.delete(&ids[..1], &opt).await;
    let after = store
        .similarity_search("capital of France", 2, &opt)
        .await
        .unwrap();
    println!("After delete: {} results", after.len());
}

#[cfg(not(feature = "in-memory"))]
fn main() {
    println!("Run: cargo run --example vector_store_in_memory --features in-memory");
}
