// Example: Agentic RAG
// This demonstrates the Agentic RAG implementation where an agent decides when to retrieve

use std::sync::Arc;

#[cfg(feature = "postgres")]
use oris::{
    embedding::openai::openai_embedder::OpenAiEmbedder,
    rag::{AgenticRAGBuilder, RetrieverInfo},
    schemas::{Document, Message},
    vectorstore::{pgvector::StoreBuilder, Retriever},
};

#[cfg(feature = "postgres")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create sample documents
    let documents = vec![
        Document::new(
            "LangChain is a framework for developing applications powered by language models.",
        ),
        Document::new(
            "RAG stands for Retrieval-Augmented Generation, combining retrieval and generation.",
        ),
        Document::new(
            "Agents can use tools to interact with external systems and retrieve information.",
        ),
    ];

    // Create vector store
    let store = StoreBuilder::new()
        .embedder(OpenAiEmbedder::default())
        .pre_delete_collection(true)
        .connection_url("postgresql://postgres:postgres@localhost:5432/postgres")
        .vector_dimensions(1536)
        .build()
        .await?;

    // Add documents to store
    use oris::vectorstore::{pgvector::PgOptions, VectorStore};
    let _ = store
        .add_documents(&documents, &PgOptions::default())
        .await?;

    // Create retriever
    let retriever: Arc<dyn oris::schemas::Retriever> =
        Arc::new(Retriever::new(store, 3));

    // Build Agentic RAG
    let agentic_rag = AgenticRAGBuilder::new()
        .with_model("gpt-4o-mini")
        .with_system_prompt(
            "You are a helpful assistant. Use the retrieve_documents tool when you need to \
             find information from the knowledge base. Always cite your sources when using retrieved information."
        )
        .with_retriever(RetrieverInfo::new(
            retriever,
            "retrieve_documents".to_string(),
            "Retrieve relevant documents from the knowledge base. Use this when you need information \
             about LangChain, RAG, or agents.".to_string(),
        ))
        .build()?;

    println!("Agentic RAG Example\n");

    // Test 1: Question that requires retrieval
    println!("Question: What is LangChain?");
    let answer = agentic_rag
        .invoke_messages(vec![Message::new_human_message("What is LangChain?")])
        .await?;
    println!("Answer: {}\n", answer);

    // Test 2: Question that might not need retrieval
    println!("Question: Hello, how are you?");
    let answer = agentic_rag
        .invoke_messages(vec![Message::new_human_message("Hello, how are you?")])
        .await?;
    println!("Answer: {}\n", answer);

    Ok(())
}

#[cfg(not(feature = "postgres"))]
fn main() {
    println!("This example requires the 'postgres' feature to be enabled.");
    println!("Please run: cargo run --example rag_agentic --features postgres");
}
