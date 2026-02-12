use async_trait::async_trait;

use super::{TextSplitter, TextSplitterError};

/// Configuration for CodeSplitter
#[derive(Debug, Clone)]
pub struct CodeSplitterOptions {
    /// Maximum chunk size (in characters)
    pub chunk_size: usize,
    /// Overlap between chunks (in characters)
    pub chunk_overlap: usize,
    /// Whether to trim whitespace from chunks
    pub trim_chunks: bool,
}

impl Default for CodeSplitterOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeSplitterOptions {
    pub fn new() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 200,
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

    pub fn with_trim_chunks(mut self, trim_chunks: bool) -> Self {
        self.trim_chunks = trim_chunks;
        self
    }
}

/// CodeSplitter splits code based on syntax tree structure
///
/// This splitter uses tree-sitter to parse code and split by
/// functions, classes, modules, etc. Requires the `tree-sitter` feature.
#[cfg(feature = "tree-sitter")]
pub struct CodeSplitter {
    language: tree_sitter::Language,
    options: CodeSplitterOptions,
}

// In tree-sitter 0.26, Language should implement Copy, but if the compiler
// doesn't recognize it, we'll handle it in the implementation

#[cfg(not(feature = "tree-sitter"))]
pub struct CodeSplitter {
    options: CodeSplitterOptions,
}

impl CodeSplitter {
    /// Create a new CodeSplitter with a tree-sitter language
    #[cfg(feature = "tree-sitter")]
    pub fn new(language: tree_sitter::Language) -> Self {
        Self::with_options(language, CodeSplitterOptions::default())
    }

    /// Create a new CodeSplitter with options
    #[cfg(feature = "tree-sitter")]
    pub fn with_options(language: tree_sitter::Language, options: CodeSplitterOptions) -> Self {
        Self { language, options }
    }

    /// Create with custom chunk size
    #[cfg(feature = "tree-sitter")]
    pub fn with_chunk_size(language: tree_sitter::Language, chunk_size: usize) -> Self {
        Self::new(language).with_chunk_size_option(chunk_size)
    }

    /// Set chunk size
    #[cfg(feature = "tree-sitter")]
    pub fn with_chunk_size_option(mut self, chunk_size: usize) -> Self {
        self.options.chunk_size = chunk_size;
        self
    }

    /// Set chunk overlap
    #[cfg(feature = "tree-sitter")]
    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.options.chunk_overlap = chunk_overlap;
        self
    }

    // Fallback implementations when tree-sitter feature is not enabled
    #[cfg(not(feature = "tree-sitter"))]
    pub fn new() -> Self {
        Self::with_options(CodeSplitterOptions::default())
    }

    #[cfg(not(feature = "tree-sitter"))]
    pub fn with_options(options: CodeSplitterOptions) -> Self {
        Self { options }
    }

    #[cfg(not(feature = "tree-sitter"))]
    pub fn with_chunk_size(_language: (), chunk_size: usize) -> Self {
        Self::new().with_chunk_size_option(chunk_size)
    }

    #[cfg(not(feature = "tree-sitter"))]
    pub fn with_chunk_size_option(mut self, chunk_size: usize) -> Self {
        self.options.chunk_size = chunk_size;
        self
    }

    #[cfg(not(feature = "tree-sitter"))]
    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.options.chunk_overlap = chunk_overlap;
        self
    }
}

#[async_trait]
impl TextSplitter for CodeSplitter {
    async fn split_text(&self, text: &str) -> Result<Vec<String>, TextSplitterError> {
        if text.is_empty() {
            return Ok(vec![]);
        }

        if self.options.chunk_size == 0 {
            return Err(TextSplitterError::InvalidSplitterOptions);
        }

        #[cfg(feature = "tree-sitter")]
        {
            use text_splitter::ChunkConfig;
            use text_splitter::CodeSplitter as TextSplitterCodeSplitter;

            let chunk_config = ChunkConfig::new(self.options.chunk_size)
                .with_trim(self.options.trim_chunks)
                .with_overlap(self.options.chunk_overlap)
                .map_err(|_| TextSplitterError::InvalidSplitterOptions)?;

            // text-splitter 0.29 CodeSplitter::new accepts impl Into<Language>
            // Language in tree-sitter 0.26 should be Copy, but to be safe we clone it
            let language = self.language.clone();
            let splitter = TextSplitterCodeSplitter::new(language, chunk_config)
                .map_err(|e| TextSplitterError::CodeParseError(format!("{}", e)))?;

            let chunks: Vec<String> = splitter.chunks(text).map(|x| x.to_string()).collect();

            Ok(chunks)
        }

        #[cfg(not(feature = "tree-sitter"))]
        {
            Err(TextSplitterError::OtherError(
                "CodeSplitter requires the 'tree-sitter' feature to be enabled.".to_string(),
            ))
        }
    }
}
