//! Example: Using WikipediaRetriever to retrieve Wikipedia articles
//!
//! This example demonstrates how to use the WikipediaRetriever to search
//! and retrieve articles from Wikipedia.

use oris::{
    chain::{Chain, ConversationalRetrieverChainBuilder},
    llm::{OpenAI, OpenAIModel},
    memory::SimpleMemory,
    prompt_args,
    retrievers::WikipediaRetriever,
    schemas::Retriever,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create Wikipedia retriever
    let wikipedia_retriever = WikipediaRetriever::new()
        .with_language("en")
        .with_max_docs(3);

    // Wrap in Retriever trait object
    let retriever: Box<dyn Retriever> = Box::new(wikipedia_retriever);

    // Create LLM
    let llm = OpenAI::default().with_model(OpenAIModel::Gpt35.to_string());

    // Create conversational retriever chain
    let chain = ConversationalRetrieverChainBuilder::new()
        .llm(llm)
        .retriever(retriever)
        .memory(SimpleMemory::new().into())
        .rephrase_question(true)
        .build()?;

    // Query
    let result = chain
        .invoke(prompt_args! {
            "question" => "What is Rust programming language?",
        })
        .await?;

    println!("Answer: {}", result);

    Ok(())
}
