#[cfg(feature = "ollama")]
pub mod client;
#[cfg(feature = "ollama")]
pub use client::Ollama;

pub mod openai;
