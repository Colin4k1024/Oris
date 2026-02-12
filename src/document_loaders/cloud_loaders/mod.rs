#[cfg(feature = "aws-s3")]
mod aws_s3_loader;
#[cfg(feature = "aws-s3")]
pub use aws_s3_loader::*;

// Azure Blob Storage, GCS, and Google Drive loaders would be similar
// They require additional dependencies and API clients
