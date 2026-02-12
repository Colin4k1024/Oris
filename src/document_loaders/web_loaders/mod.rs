mod web_base_loader;
pub use web_base_loader::*;

mod recursive_url_loader;
pub use recursive_url_loader::*;

#[cfg(feature = "xml")]
mod sitemap_loader;
#[cfg(feature = "xml")]
pub use sitemap_loader::*;
