#[cfg(feature = "excel")]
mod excel_loader;
#[cfg(feature = "excel")]
pub use excel_loader::*;

// Word and PowerPoint can use PandocLoader which already exists
// But we can add dedicated loaders if needed
