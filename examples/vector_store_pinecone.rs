// To run: cargo run --example vector_store_pinecone --features pinecone
// Requires: PINECONE_API_KEY, PINECONE_ENV (e.g. us-east1-gcp), and an existing index.
// OPENAI_API_KEY for embedder.

#[cfg(feature = "pinecone")]
use oris::{
    embedding::openai::openai_embedder::OpenAiEmbedder, schemas::Document,
    vectorstore::pinecone::StoreBuilder, vectorstore::VecStoreOptions, vectorstore::VectorStore,
};

#[cfg(feature = "pinecone")]
#[tokio::main]
async fn main() {
    let api_key = std::env::var("PINECONE_API_KEY").expect("PINECONE_API_KEY");
    let environment = std::env::var("PINECONE_ENV").unwrap_or_else(|_| "us-east1-gcp".into());
    let index_name = std::env::var("PINECONE_INDEX").unwrap_or_else(|_| "oris".into());

    let store = StoreBuilder::new()
        .api_key(api_key)
        .environment(environment)
        .index_name(index_name)
        .embedder(OpenAiEmbedder::default())
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

#[cfg(not(feature = "pinecone"))]
fn main() {
    println!("Run: cargo run --example vector_store_pinecone --features pinecone");
}
