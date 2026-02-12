// Example: 2-Step RAG
// This demonstrates the optimized 2-Step RAG implementation

#[cfg(feature = "postgres")]
use oris::{
    embedding::openai::openai_embedder::OpenAiEmbedder,
    llm::openai::{OpenAI, OpenAIModel},
    memory::SimpleMemory,
    rag::two_step::TwoStepRAGBuilder,
    schemas::Document,
    vectorstore::{pgvector::StoreBuilder, Retriever},
};

#[cfg(feature = "postgres")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create sample documents
    let documents = vec![
        Document::new("Rust is a systems programming language focused on safety and performance."),
        Document::new("Python is a high-level interpreted language known for its simplicity."),
        Document::new("JavaScript is a scripting language primarily used for web development."),
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
    let retriever: Box<dyn oris::schemas::Retriever> =
        Box::new(Retriever::new(store, 3));

    // Create LLM
    let llm = OpenAI::default().with_model(OpenAIModel::Gpt4oMini.to_string());

    // Build 2-Step RAG
    let rag = TwoStepRAGBuilder::new()
        .with_llm(llm)
        .with_retriever(retriever)
        .with_memory(SimpleMemory::new().into())
        .with_rephrase_question(true)
        .with_return_source_documents(true)
        .build()?;

    println!("2-Step RAG Example\n");
    println!("Question: What is Rust?");
    let answer = rag.invoke("What is Rust?").await?;
    println!("Answer: {}\n", answer);

    println!("Question: Compare Rust and Python");
    let answer = rag.invoke("Compare Rust and Python").await?;
    println!("Answer: {}\n", answer);

    Ok(())
}

#[cfg(not(feature = "postgres"))]
fn main() {
    println!("This example requires the 'postgres' feature to be enabled.");
    println!("Please run: cargo run --example rag_two_step --features postgres");
}
