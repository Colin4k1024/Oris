#[cfg(feature = "gemini")]
pub mod client;
#[cfg(feature = "gemini")]
pub mod error;
#[cfg(feature = "gemini")]
pub mod models;

#[cfg(feature = "gemini")]
pub use client::Gemini;
#[cfg(feature = "gemini")]
pub use error::GeminiError;
