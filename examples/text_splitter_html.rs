//! Example: Using HTMLSplitter
//!
//! This example demonstrates how to use the HTMLSplitter to split HTML documents
//! based on HTML tag structure.

use oris::text_splitter::{HTMLSplitter, TextSplitter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an HTML splitter
    let splitter = HTMLSplitter::new()
        .with_chunk_size_option(200)
        .with_chunk_overlap(50)
        .with_split_tags(vec!["p".to_string(), "div".to_string()]);

    // Sample HTML content
    let html = r#"
<html>
<body>
    <h1>Title</h1>
    <p>This is the first paragraph with some content.</p>
    <p>This is the second paragraph with more content.</p>
    <div>
        <p>This is a paragraph inside a div.</p>
        <p>Another paragraph in the same div.</p>
    </div>
    <section>
        <p>This is a paragraph in a section.</p>
    </section>
</body>
</html>
"#;

    // Split the HTML
    let chunks = splitter.split_text(html).await?;

    println!("Number of chunks: {}", chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        println!("\n--- Chunk {} ({} chars) ---", i + 1, chunk.len());
        println!("{}", chunk);
    }

    Ok(())
}
