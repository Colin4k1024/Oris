// Example: Vector search in long-term memory
// This demonstrates using vector similarity search to find relevant memories

use std::sync::Arc;

use oris::{
    embedding::openai::openai_embedder::OpenAiEmbedder,
    tools::{
        EnhancedInMemoryStore, EnhancedInMemoryStoreConfig, EnhancedToolStore, StoreFilter,
        StoreValue,
    },
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create enhanced store with vector index
    let embedder = Arc::new(OpenAiEmbedder::default());
    let config = EnhancedInMemoryStoreConfig::new().with_vector_index(embedder.clone(), 1536);

    let store = Arc::new(EnhancedInMemoryStore::with_config(config));

    // Store some memories with different content
    let memories = vec![
        (
            "memory1",
            json!({
                "rules": [
                    "User likes short, direct language",
                    "User only speaks English & python",
                ],
                "preferences": "concise responses"
            }),
        ),
        (
            "memory2",
            json!({
                "rules": [
                    "User prefers detailed explanations",
                    "User speaks multiple languages",
                ],
                "preferences": "comprehensive answers"
            }),
        ),
        (
            "memory3",
            json!({
                "rules": [
                    "User likes code examples",
                    "User works with Rust and Python",
                ],
                "preferences": "practical examples"
            }),
        ),
    ];

    for (key, value) in memories {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("type".to_string(), json!("user_preference"));

        store
            .put_with_metadata(
                &["user_preferences"],
                key,
                StoreValue::with_metadata(value, metadata),
            )
            .await;
    }

    println!("Long-term Memory Search Example\n");

    // Example 1: Vector similarity search
    println!("1. Vector similarity search for 'language preferences':");
    let results = store
        .search(&["user_preferences"], Some("language preferences"), None, 3)
        .await?;

    for (i, result) in results.iter().enumerate() {
        println!("  Result {}: {}", i + 1, result.value);
    }

    // Example 2: Search with filter
    println!("\n2. Search with content filter:");
    let filter = StoreFilter::content_contains("preferences".to_string(), "concise".to_string());
    let results = store
        .search(&["user_preferences"], None, Some(&filter), 5)
        .await?;

    for (i, result) in results.iter().enumerate() {
        println!("  Result {}: {}", i + 1, result.value);
    }

    // Example 3: Search with metadata filter
    println!("\n3. Search with metadata filter:");
    let filter = StoreFilter::metadata_equals("type".to_string(), json!("user_preference"));
    let results = store
        .search_by_filter(&["user_preferences"], &filter, 10)
        .await?;

    println!(
        "  Found {} memories with type 'user_preference'",
        results.len()
    );

    // Example 4: Combined search (vector + filter)
    println!("\n4. Combined vector search with filter:");
    let filter = StoreFilter::content_contains("preferences".to_string(), "examples".to_string());
    let results = store
        .search(
            &["user_preferences"],
            Some("code and programming"),
            Some(&filter),
            5,
        )
        .await?;

    for (i, result) in results.iter().enumerate() {
        println!("  Result {}: {}", i + 1, result.value);
    }

    Ok(())
}
