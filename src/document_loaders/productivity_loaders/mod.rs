#[cfg(feature = "github")]
mod github_loader;
#[cfg(feature = "github")]
pub use github_loader::*;

// Notion and Slack loaders would require their respective API clients
// They follow similar patterns but need specific SDKs
