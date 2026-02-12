//! Example: Using MergerRetriever to combine results from multiple retrievers
//!
//! This example demonstrates how to use the MergerRetriever to combine
//! results from multiple retrievers using different merge strategies.

use std::sync::Arc;

use oris::{
    chain::{Chain, ConversationalRetrieverChainBuilder},
    llm::{OpenAI, OpenAIModel},
    memory::SimpleMemory,
    prompt_args,
    retrievers::{MergeStrategy, MergerRetriever, WikipediaRetriever},
    schemas::Retriever as RetrieverTrait,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create multiple retrievers
    let wikipedia_retriever = Arc::new(WikipediaRetriever::new().with_max_docs(3));
    // You can add more retrievers here, e.g., ArxivRetriever, VectorStoreRetriever, etc.

    // Create merger retriever with Reciprocal Rank Fusion strategy
    let mut merger_retriever = MergerRetriever::new(vec![wikipedia_retriever.clone()]);
    merger_retriever.config.strategy = MergeStrategy::ReciprocalRankFusion { k: 60.0 };
    merger_retriever.config.top_k = 5;

    // Wrap in Retriever trait object
    let retriever: Box<dyn RetrieverTrait> = Box::new(merger_retriever);

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
            "question" => "What is machine learning?",
        })
        .await?;

    println!("Answer: {}", result);

    Ok(())
}
