#[cfg(feature = "mistralai")]
pub mod client;
#[cfg(feature = "mistralai")]
pub mod error;
#[cfg(feature = "mistralai")]
pub mod models;

#[cfg(feature = "mistralai")]
pub use client::MistralAI;
#[cfg(feature = "mistralai")]
pub use error::MistralAIError;
