//! Example: Using RecursiveCharacterTextSplitter
//!
//! This example demonstrates how to use the RecursiveCharacterTextSplitter,
//! which is the recommended text splitter for most use cases.

use oris::{
    schemas::Document,
    text_splitter::{RecursiveCharacterTextSplitter, TextSplitter},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a recursive character splitter
    let splitter = RecursiveCharacterTextSplitter::new()
        .with_chunk_size_option(100)
        .with_chunk_overlap(20);

    // Sample text with paragraphs, sentences, and words
    let text = r#"
This is the first paragraph. It contains multiple sentences.
Each sentence provides information about the topic.

This is the second paragraph. It also has multiple sentences.
The splitter will try to keep paragraphs together first.

This is the third paragraph. If paragraphs are too large,
it will split by sentences, then by words, and finally by characters.
"#;

    // Split the text
    let chunks = splitter.split_text(text).await?;

    println!("Number of chunks: {}", chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        println!("\n--- Chunk {} ({} chars) ---", i + 1, chunk.len());
        println!("{}", chunk);
    }

    // Split documents
    let documents = vec![
        Document::new("First document content here."),
        Document::new("Second document content here."),
    ];

    let split_docs = splitter.split_documents(&documents).await?;
    println!(
        "\nSplit {} documents into {} chunks",
        documents.len(),
        split_docs.len()
    );

    Ok(())
}
