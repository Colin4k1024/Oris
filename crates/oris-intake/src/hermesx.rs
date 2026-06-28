//! Hermesx execution event ingestion with Ed25519 signature verification.
//!
//! Accepts `POST /v1/ingest/hermesx/execution-event` payloads from the Hermesx
//! execution platform. Only **failed** execution events are ingested — success
//! events are rejected with `204 No Content` (acknowledged but not processed).
//!
//! ## Signature Verification
//!
//! Each request must include:
//! - `X-Hermesx-Signature`: base64-encoded Ed25519 signature over the raw body
//! - `X-Hermesx-Key-Id`: hex-encoded 32-byte Ed25519 public key
//!
//! If verification is enabled (a verifier is configured), requests with invalid
//! or missing signatures are rejected with `403 Forbidden`.

use serde::{Deserialize, Serialize};

use crate::{IntakeError, IntakeEvent, IntakeResult, IntakeSourceType, IssueSeverity};

/// A Hermesx execution event as received from the platform.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HermesxExecutionEvent {
    /// Unique execution ID from Hermesx
    pub execution_id: String,
    /// Task or workflow name
    pub task_name: String,
    /// Whether the execution succeeded
    pub success: bool,
    /// Exit code (if applicable)
    pub exit_code: Option<i32>,
    /// Error message (if failed)
    pub error_message: Option<String>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Timestamp (Unix millis)
    pub timestamp_ms: i64,
    /// Optional execution context/environment
    pub environment: Option<String>,
    /// Optional stderr output (truncated)
    pub stderr: Option<String>,
}

/// Hermesx intake source that converts execution events into IntakeEvents.
pub struct HermesxIntakeSource;

impl HermesxIntakeSource {
    /// Parse and convert a hermesx execution event payload.
    ///
    /// Returns `Ok(None)` if the event is a success (should be skipped).
    /// Returns `Ok(Some(event))` for failure events.
    /// Returns `Err` if the payload cannot be parsed.
    pub fn process_event(&self, payload: &[u8]) -> IntakeResult<Option<IntakeEvent>> {
        let event: HermesxExecutionEvent = serde_json::from_slice(payload)
            .map_err(|e| IntakeError::ParseError(format!("invalid hermesx payload: {e}")))?;

        if event.success {
            return Ok(None);
        }

        let severity = match event.exit_code {
            Some(code) if code >= 128 => IssueSeverity::Critical, // signal kill
            Some(_) => IssueSeverity::High,
            None => IssueSeverity::Medium,
        };

        let description = build_description(&event);

        let mut signals = vec![format!("hermesx:task:{}", event.task_name)];
        if let Some(code) = event.exit_code {
            signals.push(format!("exit_code:{code}"));
        }

        let intake_event = IntakeEvent {
            event_id: crate::generate_intake_id("hmx"),
            source_type: IntakeSourceType::Hermesx,
            source_event_id: Some(event.execution_id),
            title: format!("Hermesx execution failed: {}", event.task_name),
            description,
            severity,
            signals,
            raw_payload: Some(String::from_utf8_lossy(payload).to_string()),
            timestamp_ms: event.timestamp_ms,
        };

        Ok(Some(intake_event))
    }
}

fn build_description(event: &HermesxExecutionEvent) -> String {
    let mut parts = vec![format!(
        "Task '{}' failed after {}ms",
        event.task_name, event.duration_ms
    )];

    if let Some(code) = event.exit_code {
        parts.push(format!("Exit code: {code}"));
    }
    if let Some(ref msg) = event.error_message {
        parts.push(format!("Error: {msg}"));
    }
    if let Some(ref env) = event.environment {
        parts.push(format!("Environment: {env}"));
    }

    parts.join(". ")
}

/// Ed25519 signature verifier for Hermesx requests.
pub struct HermesxVerifier {
    /// Trusted public keys (hex-encoded 32-byte keys)
    trusted_keys: Vec<String>,
}

impl HermesxVerifier {
    /// Create a verifier with a set of trusted public keys (hex-encoded).
    pub fn new(trusted_keys: Vec<String>) -> Self {
        Self { trusted_keys }
    }

    /// Verify that `signature_b64` (base64) over `body` matches `key_id` (hex public key).
    pub fn verify(
        &self,
        key_id: &str,
        signature_b64: &str,
        body: &[u8],
    ) -> Result<(), VerifyError> {
        if !self.trusted_keys.iter().any(|k| k == key_id) {
            return Err(VerifyError::UntrustedKey);
        }

        let sig_bytes = base64_decode(signature_b64).map_err(|_| VerifyError::InvalidSignature)?;
        let sig_array: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| VerifyError::InvalidSignature)?;

        let key_bytes = hex::decode(key_id).map_err(|_| VerifyError::InvalidKey)?;
        let key_array: [u8; 32] = key_bytes.try_into().map_err(|_| VerifyError::InvalidKey)?;

        let public_key = ed25519_dalek::VerifyingKey::from_bytes(&key_array)
            .map_err(|_| VerifyError::InvalidKey)?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig_array);

        use ed25519_dalek::Verifier;
        public_key
            .verify(body, &signature)
            .map_err(|_| VerifyError::SignatureMismatch)
    }
}

/// Errors from signature verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    UntrustedKey,
    InvalidSignature,
    InvalidKey,
    SignatureMismatch,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UntrustedKey => write!(f, "key not in trusted set"),
            Self::InvalidSignature => write!(f, "invalid signature encoding"),
            Self::InvalidKey => write!(f, "invalid public key"),
            Self::SignatureMismatch => write!(f, "signature verification failed"),
        }
    }
}

impl std::error::Error for VerifyError {}

fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    // Simple base64 decoder (standard alphabet, no padding required)
    use std::io::Read;
    let mut decoder = base64_reader(input.as_bytes());
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).map_err(|_| ())?;
    Ok(buf)
}

fn base64_reader(input: &[u8]) -> impl std::io::Read + '_ {
    Base64Decoder {
        input,
        pos: 0,
        buf: [0; 3],
        buf_len: 0,
        buf_pos: 0,
    }
}

struct Base64Decoder<'a> {
    input: &'a [u8],
    pos: usize,
    buf: [u8; 3],
    buf_len: usize,
    buf_pos: usize,
}

impl std::io::Read for Base64Decoder<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let mut written = 0;
        while written < out.len() {
            if self.buf_pos < self.buf_len {
                out[written] = self.buf[self.buf_pos];
                self.buf_pos += 1;
                written += 1;
                continue;
            }
            // Decode next 4 chars
            let mut quad = [0u8; 4];
            let mut qi = 0;
            while qi < 4 {
                if self.pos >= self.input.len() {
                    if qi == 0 {
                        return Ok(written);
                    }
                    // Pad remaining
                    while qi < 4 {
                        quad[qi] = 64;
                        qi += 1;
                    } // '=' sentinel
                    break;
                }
                let b = self.input[self.pos];
                self.pos += 1;
                if b == b'=' {
                    quad[qi] = 64;
                    qi += 1;
                    continue;
                }
                let val = match b {
                    b'A'..=b'Z' => b - b'A',
                    b'a'..=b'z' => b - b'a' + 26,
                    b'0'..=b'9' => b - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    b'-' => 62, // URL-safe
                    b'_' => 63, // URL-safe
                    b'\n' | b'\r' | b' ' => continue,
                    _ => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "bad base64",
                        ))
                    }
                };
                quad[qi] = val;
                qi += 1;
            }
            let n = if quad[2] == 64 {
                1
            } else if quad[3] == 64 {
                2
            } else {
                3
            };
            let combined = ((quad[0] as u32) << 18)
                | ((quad[1] as u32) << 12)
                | (((quad[2] & 0x3f) as u32) << 6)
                | ((quad[3] & 0x3f) as u32);
            self.buf[0] = (combined >> 16) as u8;
            self.buf[1] = (combined >> 8) as u8;
            self.buf[2] = combined as u8;
            self.buf_len = n;
            self.buf_pos = 0;
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_failed_event() -> HermesxExecutionEvent {
        HermesxExecutionEvent {
            execution_id: "exec-001".to_string(),
            task_name: "build-service-a".to_string(),
            success: false,
            exit_code: Some(1),
            error_message: Some("compilation error".to_string()),
            duration_ms: 45_000,
            timestamp_ms: 1719500000000,
            environment: Some("staging".to_string()),
            stderr: Some("error[E0308]: mismatched types".to_string()),
        }
    }

    fn make_success_event() -> HermesxExecutionEvent {
        HermesxExecutionEvent {
            execution_id: "exec-002".to_string(),
            task_name: "build-service-a".to_string(),
            success: true,
            exit_code: Some(0),
            error_message: None,
            duration_ms: 30_000,
            timestamp_ms: 1719500000000,
            environment: None,
            stderr: None,
        }
    }

    #[test]
    fn success_events_are_skipped() {
        let source = HermesxIntakeSource;
        let payload = serde_json::to_vec(&make_success_event()).unwrap();
        let result = source.process_event(&payload).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn failed_event_produces_intake_event() {
        let source = HermesxIntakeSource;
        let payload = serde_json::to_vec(&make_failed_event()).unwrap();
        let result = source.process_event(&payload).unwrap();
        let event = result.expect("should produce event for failure");

        assert_eq!(event.source_type, IntakeSourceType::Hermesx);
        assert_eq!(event.severity, IssueSeverity::High);
        assert!(event.title.contains("build-service-a"));
        assert!(event
            .signals
            .contains(&"hermesx:task:build-service-a".to_string()));
        assert!(event.signals.contains(&"exit_code:1".to_string()));
    }

    #[test]
    fn signal_kill_exit_code_maps_to_critical() {
        let mut ev = make_failed_event();
        ev.exit_code = Some(137); // SIGKILL
        let source = HermesxIntakeSource;
        let payload = serde_json::to_vec(&ev).unwrap();
        let result = source.process_event(&payload).unwrap().unwrap();
        assert_eq!(result.severity, IssueSeverity::Critical);
    }

    #[test]
    fn no_exit_code_maps_to_medium() {
        let mut ev = make_failed_event();
        ev.exit_code = None;
        let source = HermesxIntakeSource;
        let payload = serde_json::to_vec(&ev).unwrap();
        let result = source.process_event(&payload).unwrap().unwrap();
        assert_eq!(result.severity, IssueSeverity::Medium);
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let source = HermesxIntakeSource;
        let result = source.process_event(b"not json");
        assert!(result.is_err());
    }

    #[test]
    fn verifier_rejects_untrusted_key() {
        let verifier = HermesxVerifier::new(vec!["aabbccdd".to_string()]);
        let result = verifier.verify("deadbeef", "c2lnbmF0dXJl", b"body");
        assert_eq!(result, Err(VerifyError::UntrustedKey));
    }

    #[test]
    fn verifier_rejects_invalid_signature_encoding() {
        let verifier = HermesxVerifier::new(vec!["aa".repeat(32)]);
        let key = "aa".repeat(32);
        let result = verifier.verify(&key, "!!!invalid!!!", b"body");
        assert_eq!(result, Err(VerifyError::InvalidSignature));
    }

    #[test]
    fn verifier_validates_real_signature() {
        use ed25519_dalek::{Signer, SigningKey};

        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let public_key = signing_key.verifying_key();
        let key_hex = hex::encode(public_key.as_bytes());

        let body = b"test payload";
        let signature = signing_key.sign(body);
        let sig_b64 = base64_encode(&signature.to_bytes());

        let verifier = HermesxVerifier::new(vec![key_hex.clone()]);
        assert!(verifier.verify(&key_hex, &sig_b64, body).is_ok());
    }

    #[test]
    fn verifier_rejects_wrong_body() {
        use ed25519_dalek::{Signer, SigningKey};

        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let public_key = signing_key.verifying_key();
        let key_hex = hex::encode(public_key.as_bytes());

        let signature = signing_key.sign(b"correct body");
        let sig_b64 = base64_encode(&signature.to_bytes());

        let verifier = HermesxVerifier::new(vec![key_hex.clone()]);
        let result = verifier.verify(&key_hex, &sig_b64, b"wrong body");
        assert_eq!(result, Err(VerifyError::SignatureMismatch));
    }

    fn base64_encode(data: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            result.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
            result.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                result.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
            } else {
                result.push('=');
            }
            if chunk.len() > 2 {
                result.push(ALPHABET[(triple & 0x3F) as usize] as char);
            } else {
                result.push('=');
            }
        }
        result
    }
}
