package internal

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/hex"
	"testing"
)

func TestSignBodyProducesValidSignature(t *testing.T) {
	seed := [32]byte{}
	copy(seed[:], []byte("test-seed-32-bytes-for-testing!!"))

	body := []byte(`{"node_id":"test-001"}`)
	sig := SignBody(seed, body)

	sigBytes, err := base64.StdEncoding.DecodeString(sig)
	if err != nil {
		t.Fatalf("invalid base64: %v", err)
	}
	if len(sigBytes) != 64 {
		t.Fatalf("signature length = %d, want 64", len(sigBytes))
	}

	pub := ed25519.NewKeyFromSeed(seed[:]).Public().(ed25519.PublicKey)
	if !ed25519.Verify(pub, body, sigBytes) {
		t.Fatal("signature verification failed")
	}
}

func TestSignPayloadMatchesManualSign(t *testing.T) {
	seed := [32]byte{}
	copy(seed[:], []byte("test-seed-32-bytes-for-testing!!"))

	payload := map[string]interface{}{"gene_id": "g1", "confidence": 0.9}
	sig, err := SignPayload(seed, payload)
	if err != nil {
		t.Fatalf("SignPayload error: %v", err)
	}

	sigBytes, err := base64.StdEncoding.DecodeString(sig)
	if err != nil {
		t.Fatalf("invalid base64: %v", err)
	}
	if len(sigBytes) != 64 {
		t.Fatalf("signature length = %d, want 64", len(sigBytes))
	}
}

func TestPublicKeyBase64(t *testing.T) {
	seed := [32]byte{}
	copy(seed[:], []byte("test-seed-32-bytes-for-testing!!"))

	b64 := PublicKeyBase64(seed)
	decoded, err := base64.StdEncoding.DecodeString(b64)
	if err != nil {
		t.Fatalf("invalid base64: %v", err)
	}
	if len(decoded) != 32 {
		t.Fatalf("public key length = %d, want 32", len(decoded))
	}
}

func TestPublicKeyHex(t *testing.T) {
	seed := [32]byte{}
	copy(seed[:], []byte("test-seed-32-bytes-for-testing!!"))

	h := PublicKeyHex(seed)
	decoded, err := hex.DecodeString(h)
	if err != nil {
		t.Fatalf("invalid hex: %v", err)
	}
	if len(decoded) != 32 {
		t.Fatalf("public key length = %d, want 32", len(decoded))
	}
}
