// Example: Hybrid RAG
// This demonstrates the Hybrid RAG implementation with query enhancement, retrieval validation, and answer validation

#[cfg(feature = "postgres")]
use std::sync::Arc;

#[cfg(feature = "postgres")]
use oris_runtime::{
    embedding::openai::openai_embedder::OpenAiEmbedder,
    llm::openai::{OpenAI, OpenAIModel},
    memory::SimpleMemory,
    rag::{
        HybridRAGBuilder, HybridRAGConfig, LLMAnswerValidator, LLMQueryEnhancer, RelevanceValidator,
    },
    schemas::Document,
    vectorstore::{pgvector::StoreBuilder, Retriever},
};

#[cfg(feature = "postgres")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create sample documents
    let documents = vec![
        Document::new("Machine learning is a subset of artificial intelligence that enables systems to learn from data."),
        Document::new("Deep learning uses neural networks with multiple layers to model complex patterns."),
        Document::new("Natural language processing (NLP) enables computers to understand and generate human language."),
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
    use oris_runtime::vectorstore::{pgvector::PgOptions, VectorStore};
    let _ = store
        .add_documents(&documents, &PgOptions::default())
        .await?;

    // Create retriever
    let retriever: Arc<dyn oris_runtime::schemas::Retriever> = Arc::new(Retriever::new(store, 3));

    // Create LLM
    let llm = OpenAI::default().with_model(OpenAIModel::Gpt4oMini.to_string());
    let llm_box1: Box<dyn oris_runtime::language_models::llm::LLM> = Box::new(llm.clone());
    let llm_box2: Box<dyn oris_runtime::language_models::llm::LLM> = Box::new(llm.clone());

    // Create query enhancer
    let query_enhancer = Arc::new(LLMQueryEnhancer::new(llm_box1));

    // Create retrieval validator
    let retrieval_validator = Arc::new(RelevanceValidator::new().with_min_documents(1));

    // Create answer validator
    let answer_validator = Arc::new(LLMAnswerValidator::new(llm_box2));

    // Configure Hybrid RAG
    let config = HybridRAGConfig {
        max_retrieval_retries: 2,
        max_generation_retries: 2,
        enable_query_enhancement: true,
        enable_retrieval_validation: true,
        enable_answer_validation: true,
    };

    // Build Hybrid RAG
    let hybrid_rag = HybridRAGBuilder::new()
        .with_retriever(retriever)
        .with_llm(llm)
        .with_memory(SimpleMemory::new().into())
        .with_query_enhancer(query_enhancer)
        .with_retrieval_validator(retrieval_validator)
        .with_answer_validator(answer_validator)
        .with_config(config)
        .build()?;

    println!("Hybrid RAG Example\n");

    // Test with a query that will go through the full pipeline
    println!("Question: What is machine learning?");
    let answer = hybrid_rag.invoke("What is machine learning?").await?;
    println!("Answer: {}\n", answer);

    // Test with a more complex query
    println!("Question: Explain the relationship between AI, ML, and NLP");
    let answer = hybrid_rag
        .invoke("Explain the relationship between AI, ML, and NLP")
        .await?;
    println!("Answer: {}\n", answer);

    Ok(())
}

#[cfg(not(feature = "postgres"))]
fn main() {
    println!("This example requires the 'postgres' feature to be enabled.");
    println!("Please run: cargo run --example rag_hybrid --features postgres");
}
