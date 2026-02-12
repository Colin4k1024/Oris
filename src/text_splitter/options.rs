use text_splitter::{ChunkConfig, ChunkSizer};
use tiktoken_rs::{get_bpe_from_model, get_bpe_from_tokenizer, tokenizer::Tokenizer, CoreBPE};

use super::TextSplitterError;

// Options is a struct that contains options for a text splitter.
#[derive(Debug, Clone)]
pub struct SplitterOptions {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub model_name: String,
    pub encoding_name: String,
    pub trim_chunks: bool,
}

impl Default for SplitterOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitterOptions {
    pub fn new() -> Self {
        SplitterOptions {
            chunk_size: 512,
            chunk_overlap: 0,
            model_name: String::from("gpt-3.5-turbo"),
            encoding_name: String::from("cl100k_base"),
            trim_chunks: false,
        }
    }
}

// Builder pattern for Options struct
impl SplitterOptions {
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    pub fn with_chunk_overlap(mut self, chunk_overlap: usize) -> Self {
        self.chunk_overlap = chunk_overlap;
        self
    }

    pub fn with_model_name(mut self, model_name: &str) -> Self {
        self.model_name = String::from(model_name);
        self
    }

    pub fn with_encoding_name(mut self, encoding_name: &str) -> Self {
        self.encoding_name = String::from(encoding_name);
        self
    }

    pub fn with_trim_chunks(mut self, trim_chunks: bool) -> Self {
        self.trim_chunks = trim_chunks;
        self
    }

    pub fn get_tokenizer_from_str(s: &str) -> Option<Tokenizer> {
        match s.to_lowercase().as_str() {
            "cl100k_base" => Some(Tokenizer::Cl100kBase),
            "p50k_base" => Some(Tokenizer::P50kBase),
            "r50k_base" => Some(Tokenizer::R50kBase),
            "p50k_edit" => Some(Tokenizer::P50kEdit),
            "gpt2" => Some(Tokenizer::Gpt2),
            _ => None,
        }
    }
}

// Helper type that wraps CoreBPE to implement ChunkSizer
pub(crate) struct TiktokenSizer(CoreBPE);

impl ChunkSizer for TiktokenSizer {
    fn size(&self, chunk: &str) -> usize {
        // Use the CoreBPE to count tokens
        // encode_ordinary returns Vec<usize> directly
        self.0.encode_ordinary(chunk).len()
    }
}

impl TryFrom<&SplitterOptions> for ChunkConfig<TiktokenSizer> {
    type Error = TextSplitterError;

    fn try_from(options: &SplitterOptions) -> Result<Self, Self::Error> {
        let tk = if !options.encoding_name.is_empty() {
            let tokenizer = SplitterOptions::get_tokenizer_from_str(&options.encoding_name)
                .ok_or(TextSplitterError::TokenizerNotFound)?;

            get_bpe_from_tokenizer(tokenizer).map_err(|_| TextSplitterError::InvalidTokenizer)?
        } else {
            get_bpe_from_model(&options.model_name).map_err(|_| TextSplitterError::InvalidModel)?
        };

        let sizer = TiktokenSizer(tk);

        Ok(ChunkConfig::new(options.chunk_size)
            .with_sizer(sizer)
            .with_trim(options.trim_chunks)
            .with_overlap(options.chunk_overlap)?)
    }
}
