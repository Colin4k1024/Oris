#[cfg(feature = "bedrock")]
pub mod client;
#[cfg(feature = "bedrock")]
pub mod error;
#[cfg(feature = "bedrock")]
pub mod models;

#[cfg(feature = "bedrock")]
pub use client::Bedrock;
#[cfg(feature = "bedrock")]
pub use error::BedrockError;
