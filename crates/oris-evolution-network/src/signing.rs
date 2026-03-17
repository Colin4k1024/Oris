use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

use crate::EvolutionEnvelope;

#[derive(Debug)]
pub enum SigningError {
    HomeDirectoryUnavailable,
    Io(std::io::Error),
    InvalidKeyMaterial(&'static str),
    InvalidHex(hex::FromHexError),
    InvalidSignature,
    MissingSignature,
    ContentHashMismatch,
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SigningError::HomeDirectoryUnavailable => write!(f, "home directory unavailable"),
            SigningError::Io(error) => write!(f, "io error: {error}"),
            SigningError::InvalidKeyMaterial(message) => {
                write!(f, "invalid key material: {message}")
            }
            SigningError::InvalidHex(error) => write!(f, "invalid hex: {error}"),
            SigningError::InvalidSignature => write!(f, "invalid signature"),
            SigningError::MissingSignature => write!(f, "missing signature"),
            SigningError::ContentHashMismatch => write!(f, "content hash mismatch"),
        }
    }
}

impl std::error::Error for SigningError {}

impl From<std::io::Error> for SigningError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<hex::FromHexError> for SigningError {
    fn from(value: hex::FromHexError) -> Self {
        Self::InvalidHex(value)
    }
}

pub type SigningResult<T> = Result<T, SigningError>;
pub type SignedEnvelope = EvolutionEnvelope;

pub struct NodeKeypair {
    signing_key: SigningKey,
    path: PathBuf,
}

impl NodeKeypair {
    pub fn generate() -> SigningResult<Self> {
        let home = std::env::var_os("HOME").ok_or(SigningError::HomeDirectoryUnavailable)?;
        let path = PathBuf::from(home).join(".oris").join("node.key");
        Self::generate_at(path)
    }

    pub fn generate_at(path: impl AsRef<Path>) -> SigningResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret)
            .map_err(|_| SigningError::InvalidKeyMaterial("failed to generate randomness"))?;

        let signing_key = SigningKey::from_bytes(&secret);
        fs::write(&path, hex::encode(secret))?;
        Ok(Self { signing_key, path })
    }

    pub fn from_path(path: impl AsRef<Path>) -> SigningResult<Self> {
        let path = path.as_ref().to_path_buf();
        let contents = fs::read_to_string(&path)?;
        let secret = hex::decode(contents.trim())?;
        let secret: [u8; 32] = secret
            .try_into()
            .map_err(|_| SigningError::InvalidKeyMaterial("expected 32-byte secret key"))?;
        Ok(Self {
            signing_key: SigningKey::from_bytes(&secret),
            path,
        })
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.signing_key.verifying_key().to_bytes())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn sign_envelope(keypair: &NodeKeypair, envelope: &EvolutionEnvelope) -> SignedEnvelope {
    let mut signed = envelope.clone();
    signed.signature = None;
    signed.content_hash = signed.compute_content_hash();
    let signature = keypair.signing_key.sign(signed.content_hash.as_bytes());
    signed.signature = Some(hex::encode(signature.to_bytes()));
    signed
}

pub fn verify_envelope(
    public_key_hex: &str,
    signed_envelope: &SignedEnvelope,
) -> SigningResult<()> {
    if signed_envelope.compute_content_hash() != signed_envelope.content_hash {
        return Err(SigningError::ContentHashMismatch);
    }

    let signature_hex = signed_envelope
        .signature
        .as_ref()
        .ok_or(SigningError::MissingSignature)?;
    let signature_bytes = hex::decode(signature_hex)?;
    let signature_bytes: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| SigningError::InvalidSignature)?;
    let signature = Signature::from_bytes(&signature_bytes);

    let public_key_bytes = hex::decode(public_key_hex)?;
    let public_key_bytes: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| SigningError::InvalidKeyMaterial("expected 32-byte public key"))?;
    let public_key = VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|_| SigningError::InvalidKeyMaterial("invalid public key"))?;

    public_key
        .verify(signed_envelope.content_hash.as_bytes(), &signature)
        .map_err(|_| SigningError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvolutionEnvelope, NetworkAsset};
    use oris_evolution::{AssetState, Gene};

    fn sample_gene(id: &str) -> Gene {
        Gene {
            id: id.to_string(),
            signals: vec!["sig.test".to_string()],
            strategy: vec!["check signature".to_string()],
            validation: vec!["cargo test".to_string()],
            state: AssetState::Candidate,
            task_class_id: None,
        }
    }

    #[test]
    fn node_keypair_generate_persists_secret() {
        let temp_path = std::env::temp_dir().join(format!(
            "oris-node-key-{}.key",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let keypair =
            NodeKeypair::generate_at(&temp_path).expect("keypair generation should succeed");
        assert!(temp_path.exists());
        let loaded = NodeKeypair::from_path(&temp_path).expect("keypair should reload from disk");
        assert_eq!(keypair.public_key_hex(), loaded.public_key_hex());
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn sign_and_verify_round_trip_succeeds() {
        let temp_path = std::env::temp_dir().join(format!(
            "oris-node-key-{}.key",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let keypair =
            NodeKeypair::generate_at(&temp_path).expect("keypair generation should succeed");
        let envelope = EvolutionEnvelope::publish(
            "node-a",
            vec![NetworkAsset::Gene {
                gene: sample_gene("gene-sign"),
            }],
        );
        let signed = sign_envelope(&keypair, &envelope);
        assert!(verify_envelope(&keypair.public_key_hex(), &signed).is_ok());
        let _ = std::fs::remove_file(temp_path);
    }
}
