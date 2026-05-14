package experience

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestShareSendsApiKeyAndEnvelope(t *testing.T) {
	var apiKey string
	var receivedBody ShareRequest

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		apiKey = r.Header.Get("X-Api-Key")
		json.NewDecoder(r.Body).Decode(&receivedBody)
		json.NewEncoder(w).Encode(ShareResponse{GeneID: "g1", Status: "published"})
	}))
	defer srv.Close()

	seed := [32]byte{}
	copy(seed[:], []byte("test-seed-32-bytes-for-testing!!"))

	c := NewClient(Config{BaseURL: srv.URL, APIKey: "ak-123", Seed: seed, SenderID: "agent-1"})
	payload := map[string]any{"gene_id": "g1", "confidence": 0.9}
	resp, err := c.Share(context.Background(), payload)
	if err != nil {
		t.Fatalf("Share error: %v", err)
	}
	if apiKey != "ak-123" {
		t.Fatalf("X-Api-Key = %q, want 'ak-123'", apiKey)
	}
	if receivedBody.Envelope.SenderID != "agent-1" {
		t.Fatalf("sender_id = %q, want 'agent-1'", receivedBody.Envelope.SenderID)
	}
	if receivedBody.Envelope.MessageType != "publish" {
		t.Fatalf("message_type = %q, want 'publish'", receivedBody.Envelope.MessageType)
	}
	if receivedBody.Envelope.Signature == "" {
		t.Fatal("signature is empty")
	}
	if resp.GeneID != "g1" {
		t.Fatalf("GeneID = %q, want 'g1'", resp.GeneID)
	}
}

func TestFetchRequiresNoAuth(t *testing.T) {
	var authHeader string
	var apiKey string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		authHeader = r.Header.Get("Authorization")
		apiKey = r.Header.Get("X-Api-Key")
		json.NewEncoder(w).Encode(FetchResponse{Assets: []NetworkAsset{}, SyncAudit: SyncAudit{}})
	}))
	defer srv.Close()

	seed := [32]byte{}
	c := NewClient(Config{BaseURL: srv.URL, APIKey: "ak-123", Seed: seed, SenderID: "a1"})
	_, err := c.Fetch(context.Background(), &FetchQuery{Q: "bugfix", Limit: 5})
	if err != nil {
		t.Fatalf("Fetch error: %v", err)
	}
	if authHeader != "" {
		t.Fatalf("unexpected Authorization header: %q", authHeader)
	}
	if apiKey != "" {
		t.Fatalf("unexpected X-Api-Key header: %q", apiKey)
	}
}

func TestRegisterPublicKeySendsHex(t *testing.T) {
	var receivedBody RegisterPublicKeyRequest

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		json.NewDecoder(r.Body).Decode(&receivedBody)
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()

	seed := [32]byte{}
	copy(seed[:], []byte("test-seed-32-bytes-for-testing!!"))

	c := NewClient(Config{BaseURL: srv.URL, APIKey: "ak-123", Seed: seed, SenderID: "agent-1"})
	err := c.RegisterPublicKey(context.Background())
	if err != nil {
		t.Fatalf("RegisterPublicKey error: %v", err)
	}
	if receivedBody.SenderID != "agent-1" {
		t.Fatalf("sender_id = %q, want 'agent-1'", receivedBody.SenderID)
	}
	if len(receivedBody.PublicKeyHex) != 64 {
		t.Fatalf("public_key_hex length = %d, want 64 hex chars", len(receivedBody.PublicKeyHex))
	}
}
