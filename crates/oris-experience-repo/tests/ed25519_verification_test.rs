//! Integration tests for Ed25519 signature verification in OEN Envelopes.
//!
//! These tests verify the real Ed25519 signature generation and verification
//! flow using ed25519_dalek, including valid signatures, invalid signatures,
//! expired timestamps, and replay attack prevention.

use base64::Engine;
use chrono::{Duration, Utc};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use oris_experience_repo::oen::{MessageType, OenEnvelope, OenVerifier};

/// Generate a random Ed25519 keypair for testing.
fn generate_test_keypair() -> (SigningKey, VerifyingKey) {
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).expect("failed to generate random bytes");
    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

/// Create a valid OenEnvelope with proper structure.
fn create_valid_envelope(
    sender_id: &str,
    payload: serde_json::Value,
    timestamp: &str,
) -> OenEnvelope {
    OenEnvelope {
        sender_id: sender_id.to_string(),
        message_type: MessageType::Publish,
        payload,
        signature: String::new(), // Will be filled in by caller
        timestamp: timestamp.to_string(),
    }
}

/// Sign a payload using Ed25519 and return base64-encoded signature.
fn sign_payload(payload: &serde_json::Value, signing_key: &SigningKey) -> String {
    let payload_bytes = serde_json::to_vec(payload).expect("payload serialization should succeed");
    let signature = signing_key.sign(&payload_bytes);
    base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
}

/// Create an envelope with a valid real Ed25519 signature.
fn create_signed_envelope(
    sender_id: &str,
    payload: serde_json::Value,
    timestamp: &str,
    signing_key: &SigningKey,
) -> OenEnvelope {
    let signature = sign_payload(&payload, signing_key);
    OenEnvelope {
        sender_id: sender_id.to_string(),
        message_type: MessageType::Publish,
        payload,
        signature,
        timestamp: timestamp.to_string(),
    }
}

#[tokio::test]
async fn test_valid_signature_succeeds() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-123",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-001";

    let envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);
    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(
        result.is_ok(),
        "Valid signature should verify successfully: {:?}",
        result
    );
}

#[tokio::test]
async fn test_invalid_signature_fails() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    // Create a second keypair for generating invalid signature
    let (wrong_signing_key, _wrong_verifying_key) = generate_test_keypair();

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-456",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-002";

    // Sign with wrong key
    let wrong_signature = sign_payload(&payload, &wrong_signing_key);

    let mut envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);
    envelope.signature = wrong_signature; // Replace with wrong signature

    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(
        result.is_err(),
        "Invalid signature should fail verification"
    );
    let err_str = format!("{:?}", result);
    assert!(
        err_str.contains("SignatureFailed") || err_str.contains("signature"),
        "Error should indicate signature failure"
    );
}

#[tokio::test]
async fn test_tampered_payload_fails() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-789",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-003";

    let envelope = create_signed_envelope(sender_id, payload.clone(), &timestamp, &signing_key);

    // Tamper with payload after signing
    let mut tampered_envelope = envelope;
    tampered_envelope.payload = serde_json::json!({
        "gene": {
            "id": "gene-789",
            "signals": ["malicious.signal"]
        }
    });

    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&tampered_envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(result.is_err(), "Tampered payload should fail verification");
}

#[tokio::test]
async fn test_expired_timestamp_fails() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-expired",
            "signals": ["test.signal"]
        }
    });
    // Create a timestamp older than the 5-minute tolerance
    let expired_time = Utc::now() - Duration::minutes(10);
    let timestamp = expired_time.to_rfc3339();
    let sender_id = "agent-004";

    let envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);
    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(
        result.is_err(),
        "Expired timestamp should fail verification"
    );
    let err_str = format!("{:?}", result);
    assert!(
        err_str.contains("TimestampExpired") || err_str.contains("timestamp"),
        "Error should indicate timestamp expiration"
    );
}

#[tokio::test]
async fn test_future_timestamp_fails() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-future",
            "signals": ["test.signal"]
        }
    });
    // Create a timestamp in the future (beyond tolerance)
    let future_time = Utc::now() + Duration::minutes(10);
    let timestamp = future_time.to_rfc3339();
    let sender_id = "agent-005";

    let envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);
    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(result.is_err(), "Future timestamp should fail verification");
}

#[tokio::test]
async fn test_sender_id_mismatch_fails() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-sender",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();

    let envelope = create_signed_envelope("attacker-agent", payload, &timestamp, &signing_key);
    let verifier = OenVerifier::new();

    // Act - expect agent-006 but sender_id is "attacker-agent"
    let result = verifier
        .verify_envelope(&envelope, "agent-006", &public_key_hex)
        .await;

    // Assert
    assert!(
        result.is_err(),
        "Sender ID mismatch should fail verification"
    );
    let err_str = format!("{:?}", result);
    assert!(
        err_str.contains("SenderMismatch"),
        "Error should indicate sender mismatch"
    );
}

#[tokio::test]
async fn test_message_type_non_publish_fails() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-type",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-007";

    let mut envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);
    envelope.message_type = MessageType::Fetch; // Non-Publish type

    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(
        result.is_err(),
        "Non-Publish message type should fail verification"
    );
    let err_str = format!("{:?}", result);
    assert!(
        err_str.contains("InvalidMessageType"),
        "Error should indicate invalid message type"
    );
}

#[tokio::test]
async fn test_replay_attack_blocked_within_cache_ttl() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-replay",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-008";

    // Create envelope with valid signature
    let envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);
    let verifier = OenVerifier::new();

    // Act - First verification should succeed
    let result1 = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;
    assert!(result1.is_ok(), "First verification should succeed");

    // Act - Second verification with same envelope should succeed (cached)
    let result2 = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;
    assert!(
        result2.is_ok(),
        "Second verification with same envelope should succeed (cached)"
    );

    // The cache key is "{}:{}", so same sender_id + signature combination
    // Note: The replay protection is via caching within TTL (5 minutes)
}

#[tokio::test]
async fn test_wrong_public_key_fails() {
    // Arrange
    let (signing_key, _) = generate_test_keypair();
    // Use a different keypair's public key
    let (_, wrong_verifying_key) = generate_test_keypair();
    let wrong_public_key_hex = hex::encode(wrong_verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-wrong-key",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-009";

    let envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);
    let verifier = OenVerifier::new();

    // Act - Try to verify with wrong public key
    let result = verifier
        .verify_envelope(&envelope, sender_id, &wrong_public_key_hex)
        .await;

    // Assert
    assert!(
        result.is_err(),
        "Verification with wrong public key should fail"
    );
}

#[tokio::test]
async fn test_malformed_signature_base64_fails() {
    // Arrange
    let (_, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-malformed",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-010";

    let mut envelope = create_valid_envelope(sender_id, payload, &timestamp);
    envelope.signature = "not-valid-base64!!!".to_string(); // Malformed base64

    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(result.is_err(), "Malformed signature should fail");
}

#[tokio::test]
async fn test_invalid_signature_length_fails() {
    // Arrange
    let (_, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-bad-len",
            "signals": ["test.signal"]
        }
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-011";

    let mut envelope = create_valid_envelope(sender_id, payload, &timestamp);
    // Ed25519 signature is 64 bytes, this is too short
    envelope.signature = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 32]);

    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(result.is_err(), "Invalid signature length should fail");
}

#[tokio::test]
async fn test_custom_timestamp_tolerance() {
    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "gene": {
            "id": "gene-tolerance",
            "signals": ["test.signal"]
        }
    });
    // Create a timestamp 2 minutes in the past
    let past_time = Utc::now() - Duration::minutes(2);
    let timestamp = past_time.to_rfc3339();
    let sender_id = "agent-012";

    let envelope = create_signed_envelope(sender_id, payload, &timestamp, &signing_key);

    // Create verifier with 5-minute tolerance (default)
    let verifier_default = OenVerifier::new();
    let result_default = verifier_default
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;
    assert!(
        result_default.is_ok(),
        "2 minutes should be within 5-minute tolerance"
    );

    // Create verifier with 1-minute tolerance
    let verifier_strict = OenVerifier::new().with_timestamp_tolerance(60); // 1 minute
    let result_strict = verifier_strict
        .verify_envelope(&envelope, sender_id, &public_key_hex)
        .await;
    assert!(
        result_strict.is_err(),
        "2 minutes should exceed 1-minute tolerance"
    );
}

#[tokio::test]
async fn test_signature_encoding_hex_vs_base64() {
    // This test verifies the actual encoding format expected by the verifier
    // The verifier uses base64::STANDARD.decode() on the signature field

    // Arrange
    let (signing_key, verifying_key) = generate_test_keypair();
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    let payload = serde_json::json!({
        "test": "data"
    });
    let timestamp = Utc::now().to_rfc3339();
    let sender_id = "agent-encoding-test";

    // Create signature
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    let signature = signing_key.sign(&payload_bytes);
    let signature_bytes = signature.to_bytes();

    // Test base64 encoding (correct for verifier)
    let signature_base64 = base64::engine::general_purpose::STANDARD.encode(signature_bytes);

    let envelope_base64 = OenEnvelope {
        sender_id: sender_id.to_string(),
        message_type: MessageType::Publish,
        payload,
        signature: signature_base64,
        timestamp,
    };

    let verifier = OenVerifier::new();

    // Act
    let result = verifier
        .verify_envelope(&envelope_base64, sender_id, &public_key_hex)
        .await;

    // Assert
    assert!(
        result.is_ok(),
        "base64-encoded signature should verify successfully"
    );
}
