//! GEP (Genome Evolution Protocol) compatible types.
//!
//! This module provides GEP-compliant structures that extend the core evolution types.
//! Reference: https://evomap.ai/wiki#GEP-Protocol

mod capsule;
mod content_hash;
mod gene;
mod memory_graph;

pub use capsule::*;
pub use content_hash::*;
pub use gene::*;
pub use memory_graph::*;

use serde::{Deserialize, Serialize};

/// GEP Protocol schema version
pub const GEP_SCHEMA_VERSION: &str = "1.5.0";

/// Common envelope fields for all GEP asset types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GepEnvelope<T> {
    #[serde(rename = "type")]
    pub asset_type: String,
    #[serde(rename = "schema_version")]
    pub schema_version: String,
    #[serde(rename = "asset_id")]
    pub asset_id: String,
    #[serde(flatten)]
    pub data: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_version() {
        assert_eq!(GEP_SCHEMA_VERSION, "1.5.0");
    }
}
