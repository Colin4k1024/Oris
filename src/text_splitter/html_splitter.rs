use async_trait::async_trait;
use scraper::{Html, Selector};

use super::{TextSplitter, TextSplitterError};

/// Configuration for HTMLSplitter
#[derive(Debug, Clone)]
pub struct HTMLSplitterOptions {
    /// Maximum chunk size (in characters)
    pub chunk_size: usize,
    /// Overlap between chunks (in characters)
    pub chunk_overlap: usize,
    /// HTML tags to split on (e.g., ["p", "div", "section"])
    pub split_tags: Vec<String>,
    /// Whether to trim whitespace from chunks
    pub trim_chunks: bool,
}

impl Default for HTMLSplitterOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl HTMLSplitterOptions {
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 200,
            split_tags: vec![
                "p".to_string(),
                "div".to_string(),
                "section".to_string(),
                "article".to_string(),
            ],
            trim_chunks: true,
        }
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.chunk_overlap = chunk_overlap;
        self
    }

    pub fn with_split_tags(mut self, split_tags: Vec<String>) -> Self {
        self.split_tags = split_tags;
        self
    }

    pub fn with_trim_chunks(mut self, trim_chunks: bool) -> Self {
        self.trim_chunks = trim_chunks;
        self
    }
}

/// HTMLSplitter splits HTML documents based on HTML tag structure
///
/// This splitter extracts text from HTML elements and splits based on
/// specified HTML tags, preserving the document structure.
pub struct HTMLSplitter {
    options: HTMLSplitterOptions,
}

impl Default for HTMLSplitter {
    fn default() -> Self {
        Self::new()
    }
}

impl HTMLSplitter {
    /// Create a new HTMLSplitter with default options
    pub fn new() -> Self {
        Self::with_options(HTMLSplitterOptions::default())
    }

    /// Create a new HTMLSplitter with custom options
    pub fn with_options(options: HTMLSplitterOptions) -> Self {
        Self { options }
    }

    /// Create with custom chunk size
    pub fn with_chunk_size(chunk_size: usize) -> Self {
        Self::new().with_chunk_size_option(chunk_size)
    }

    /// Set chunk size
    pub fn with_chunk_size_option(mut self, chunk_size: usize) -> Self {
        self.options.chunk_size = chunk_size;
        self
    }

    /// Set chunk overlap
    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.options.chunk_overlap = chunk_overlap;
        self
    }

    /// Set split tags
    pub fn with_split_tags(mut self, split_tags: Vec<String>) -> Self {
        self.options.split_tags = split_tags;
        self
    }

    /// Extract text from HTML elements
    fn extract_text_from_html(&self, html: &str) -> Vec<String> {
        let document = Html::parse_document(html);
        let mut texts = Vec::new();

        // Try each split tag in order
        for tag in &self.options.split_tags {
            if let Ok(selector) = Selector::parse(tag) {
                let elements: Vec<_> = document.select(&selector).collect();
                if !elements.is_empty() {
                    for element in elements {
                        let text = element.text().collect::<Vec<_>>().join(" ");
                        if !text.trim().is_empty() {
                            texts.push(text);
                        }
                    }
                    break; // Use first matching tag
                }
            }
        }

        // If no tags matched, extract all text
        if texts.is_empty() {
            let body_selector =
                Selector::parse("body").unwrap_or_else(|_| Selector::parse("html").unwrap());
            if let Some(body) = document.select(&body_selector).next() {
                let text = body.text().collect::<Vec<_>>().join(" ");
                if !text.trim().is_empty() {
                    texts.push(text);
                }
            }
        }

        texts
    }

    /// Split extracted texts into chunks
    fn split_texts_into_chunks(&self, texts: Vec<String>) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();

        for text in texts {
            let text_trimmed = if self.options.trim_chunks {
                text.trim().to_string()
            } else {
                text
            };

            if text_trimmed.is_empty() {
                continue;
            }

            // Check if adding this text would exceed chunk size
            let test_chunk = if current_chunk.is_empty() {
                text_trimmed.clone()
            } else {
                format!("{}\n\n{}", current_chunk, text_trimmed)
            };

            if test_chunk.len() <= self.options.chunk_size {
                // Add to current chunk
                if current_chunk.is_empty() {
                    current_chunk = text_trimmed;
                } else {
                    current_chunk = format!("{}\n\n{}", current_chunk, text_trimmed);
                }
            } else {
                // Current chunk is full, save it and start new one
                if !current_chunk.is_empty() {
                    chunks.push(current_chunk.clone());
                }

                // If text itself is too large, split it
                if text_trimmed.len() > self.options.chunk_size {
                    let sub_chunks = self.split_large_text(&text_trimmed);
                    chunks.extend(sub_chunks);
                    current_chunk = String::new();
                } else {
                    current_chunk = text_trimmed;
                }
            }
        }

        // Add remaining chunk
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        // Apply overlap
        self.apply_overlap(chunks)
    }

    /// Split large text into smaller chunks
    fn split_large_text(&self, text: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let end = (start + self.options.chunk_size).min(text.len());
            let chunk = text[start..end].to_string();
            let trimmed = if self.options.trim_chunks {
                chunk.trim().to_string()
            } else {
                chunk
            };
            if !trimmed.is_empty() {
                chunks.push(trimmed);
            }
            start = end.saturating_sub(self.options.chunk_overlap);
        }

        chunks
    }

    /// Apply overlap between chunks
    fn apply_overlap(&self, chunks: Vec<String>) -> Vec<String> {
        if self.options.chunk_overlap == 0 || chunks.len() <= 1 {
            return chunks;
        }

        let mut overlapped = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            if i == 0 {
                overlapped.push(chunk.clone());
            } else {
                // Add overlap from previous chunk
                let prev_chunk = &chunks[i - 1];
                let overlap_start = prev_chunk.len().saturating_sub(self.options.chunk_overlap);
                let overlap_text = &prev_chunk[overlap_start..];

                let mut new_chunk = String::new();
                if !overlap_text.is_empty() {
                    new_chunk.push_str(overlap_text);
                    new_chunk.push_str("\n\n");
                }
                new_chunk.push_str(chunk);
                overlapped.push(new_chunk);
            }
        }

        overlapped
    }
}

#[async_trait]
impl TextSplitter for HTMLSplitter {
    async fn split_text(&self, text: &str) -> Result<Vec<String>, TextSplitterError> {
        if text.is_empty() {
            return Ok(vec![]);
        }

        if self.options.chunk_size == 0 {
            return Err(TextSplitterError::InvalidSplitterOptions);
        }

        // Extract text from HTML
        let texts = self.extract_text_from_html(text);

        // Split into chunks
        let chunks = self.split_texts_into_chunks(texts);

        Ok(chunks)
    }
}
