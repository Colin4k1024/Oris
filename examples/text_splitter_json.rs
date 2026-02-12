//! Example: Using JsonSplitter
//!
//! This example demonstrates how to use the JsonSplitter to split JSON documents
//! based on JSON structure (objects, arrays, etc.).

use oris::text_splitter::{JsonSplitMode, JsonSplitter, TextSplitter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a JSON splitter
    let splitter = JsonSplitter::new()
        .with_chunk_size_option(100)
        .with_chunk_overlap(20)
        .with_split_mode(JsonSplitMode::Both)
        .with_include_path(true);

    // Sample JSON content
    let json = r#"
{
    "users": [
        {"id": 1, "name": "Alice", "email": "alice@example.com"},
        {"id": 2, "name": "Bob", "email": "bob@example.com"},
        {"id": 3, "name": "Charlie", "email": "charlie@example.com"}
    ],
    "metadata": {
        "total": 3,
        "created": "2024-01-01"
    }
}
"#;

    // Split the JSON
    let chunks = splitter.split_text(json).await?;

    println!("Number of chunks: {}", chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        println!("\n--- Chunk {} ({} chars) ---", i + 1, chunk.len());
        println!("{}", chunk);
    }

    Ok(())
}
