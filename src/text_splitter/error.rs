use text_splitter::ChunkConfigError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TextSplitterError {
    #[error("Empty input text")]
    EmptyInputText,

    #[error("Mismatch metadata and text")]
    MetadataTextMismatch,

    #[error("Tokenizer not found")]
    TokenizerNotFound,

    #[error("Tokenizer creation failed due to invalid tokenizer")]
    InvalidTokenizer,

    #[error("Tokenizer creation failed due to invalid model")]
    InvalidModel,

    #[error("Invalid chunk overlap and size")]
    InvalidSplitterOptions,

    #[error("HTML parse error: {0}")]
    HtmlParseError(String),

    #[error("JSON parse error: {0}")]
    JsonParseError(String),

    #[error("Code parse error: {0}")]
    CodeParseError(String),

    #[error("Error: {0}")]
    OtherError(String),
}

impl From<ChunkConfigError> for TextSplitterError {
    fn from(_: ChunkConfigError) -> Self {
        Self::InvalidSplitterOptions
    }
}
