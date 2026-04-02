//! Content-addressable identity using SHA-256.
//!
//! Every GEP asset has a deterministic asset_id computed from its content,
//! enabling deduplication and tamper detection.

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AssetIdError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Invalid asset ID format")]
    InvalidFormat,
}

/// Compute the SHA-256 asset ID for a serializable object
/// The asset_id field is excluded from the hash computation
pub fn compute_asset_id<T: Serialize>(
    obj: &T,
    exclude_fields: &[&str],
) -> Result<String, AssetIdError> {
    let json = canonicalize_json(obj, exclude_fields)?;
    let hash = compute_sha256(&json);
    Ok(format!("sha256:{}", hash))
}

/// Compute SHA-256 hash of a string
pub fn compute_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Hex encode bytes
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Canonicalize JSON by sorting keys and preserving array order
/// This ensures deterministic hashing
fn canonicalize_json<T: Serialize>(
    obj: T,
    exclude_fields: &[&str],
) -> Result<String, AssetIdError> {
    let value =
        serde_json::to_value(obj).map_err(|e| AssetIdError::Serialization(e.to_string()))?;

    let canonical = canonicalize_value(&value, exclude_fields);

    serde_json::to_string(&canonical).map_err(|e| AssetIdError::Serialization(e.to_string()))
}

/// Recursively canonicalize a JSON value
fn canonicalize_value(value: &serde_json::Value, exclude_fields: &[&str]) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<_> = map.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));

            let mut result = serde_json::Map::new();
            for (key, val) in sorted {
                if exclude_fields.contains(&key.as_str()) {
                    continue;
                }
                result.insert(key.clone(), canonicalize_value(val, exclude_fields));
            }
            serde_json::Value::Object(result)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|v| canonicalize_value(v, exclude_fields))
                .collect(),
        ),
        // Primitives: convert non-finite numbers to null
        serde_json::Value::Number(n) => {
            // Check if number is finite by attempting to convert to f64
            if n.as_f64().map(|f| f.is_finite()).unwrap_or(false) {
                value.clone()
            } else {
                serde_json::Value::Null
            }
        }
        _ => value.clone(),
    }
}

/// Verify that a claimed asset_id matches the computed hash
pub fn verify_asset_id<T: Serialize>(
    obj: &T,
    claimed_id: &str,
    exclude_fields: &[&str],
) -> Result<bool, AssetIdError> {
    if !claimed_id.starts_with("sha256:") {
        return Err(AssetIdError::InvalidFormat);
    }

    let computed = compute_asset_id(obj, exclude_fields)?;
    Ok(claimed_id == computed)
}

/// Parse asset_id and return the hash portion
pub fn parse_asset_id(asset_id: &str) -> Result<String, AssetIdError> {
    if let Some(hash) = asset_id.strip_prefix("sha256:") {
        if hash.len() == 64 {
            Ok(hash.to_string())
        } else {
            Err(AssetIdError::InvalidFormat)
        }
    } else {
        Err(AssetIdError::InvalidFormat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestAsset {
        id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        optional: Option<String>,
    }

    #[test]
    fn test_compute_asset_id() {
        let asset = TestAsset {
            id: "test-1".to_string(),
            name: "Test Asset".to_string(),
            optional: None,
        };

        let asset_id = compute_asset_id(&asset, &["asset_id"]).unwrap();
        assert!(asset_id.starts_with("sha256:"));
        assert_eq!(asset_id.len(), 7 + 64);
    }

    #[test]
    fn test_deterministic() {
        let asset = TestAsset {
            id: "test-2".to_string(),
            name: "Deterministic".to_string(),
            optional: None,
        };

        let id1 = compute_asset_id(&asset, &["asset_id"]).unwrap();
        let id2 = compute_asset_id(&asset, &["asset_id"]).unwrap();

        assert_eq!(id1, id2);
    }

    #[test]
    fn test_different_content_different_hash() {
        let asset1 = TestAsset {
            id: "test-3".to_string(),
            name: "Name A".to_string(),
            optional: None,
        };

        let asset2 = TestAsset {
            id: "test-4".to_string(),
            name: "Name B".to_string(),
            optional: None,
        };

        let id1 = compute_asset_id(&asset1, &["asset_id"]).unwrap();
        let id2 = compute_asset_id(&asset2, &["asset_id"]).unwrap();

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_verify_asset_id() {
        let asset = TestAsset {
            id: "test-5".to_string(),
            name: "Verify Me".to_string(),
            optional: None,
        };

        let claimed = compute_asset_id(&asset, &["asset_id"]).unwrap();
        let valid = verify_asset_id(&asset, &claimed, &["asset_id"]).unwrap();

        assert!(valid);
    }

    #[test]
    fn test_canonicalize_object() {
        #[derive(Serialize)]
        struct Unordered {
            z: String,
            a: String,
            m: Vec<u32>,
        }

        let obj = Unordered {
            z: "z first".to_string(),
            a: "a second".to_string(),
            m: vec![3, 1, 2],
        };

        let json = canonicalize_json(&obj, &[]).unwrap();

        // Keys should be sorted: a, m, z
        assert!(json.find("\"a\":").unwrap() < json.find("\"z\":").unwrap());
    }

    #[test]
    fn test_parse_asset_id() {
        // Create a valid 64-char hex string
        let valid = "sha256:".to_string() + &"a".repeat(64);
        assert_eq!(parse_asset_id(&valid).unwrap().len(), 64);

        assert!(parse_asset_id("invalid").is_err());
        assert!(parse_asset_id("sha256:short").is_err());
    }
}
