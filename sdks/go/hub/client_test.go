package hub

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestRegisterSendsSignatureHeader(t *testing.T) {
	var receivedSig string
	var receivedBody map[string]any

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		receivedSig = r.Header.Get("X-OEN-Signature")
		json.NewDecoder(r.Body).Decode(&receivedBody)
		json.NewEncoder(w).Encode(RegisterResponse{
			NodeID:                   "node-1",
			HeartbeatIntervalSeconds: 30,
			TTLSeconds:               90,
		})
	}))
	defer srv.Close()

	seed := [32]byte{}
	copy(seed[:], []byte("test-seed-32-bytes-for-testing!!"))

	c := NewClient(Config{BaseURL: srv.URL, APIKey: "key", Seed: seed, NodeID: "node-1"})
	resp, err := c.Register(context.Background(), &RegisterRequest{
		NodeID:       "node-1",
		Endpoint:     "https://n1.example.com",
		Capabilities: []string{"evolution"},
		Version:      "0.61.0",
	})
	if err != nil {
		t.Fatalf("Register error: %v", err)
	}
	if receivedSig == "" {
		t.Fatal("X-OEN-Signature header missing")
	}
	if resp.HeartbeatIntervalSeconds != 30 {
		t.Fatalf("HeartbeatIntervalSeconds = %d, want 30", resp.HeartbeatIntervalSeconds)
	}
	if receivedBody["public_key"] == nil || receivedBody["public_key"] == "" {
		t.Fatal("public_key missing from request body")
	}
}

func TestDiscoverUsesBearerAuth(t *testing.T) {
	var authHeader string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		authHeader = r.Header.Get("Authorization")
		json.NewEncoder(w).Encode(DiscoveryResult{Nodes: []NodeInfo{}, Total: 0})
	}))
	defer srv.Close()

	seed := [32]byte{}
	c := NewClient(Config{BaseURL: srv.URL, APIKey: "my-api-key", Seed: seed, NodeID: "node-1"})
	_, err := c.Discover(context.Background(), &DiscoveryQuery{Capabilities: []string{"evolution"}})
	if err != nil {
		t.Fatalf("Discover error: %v", err)
	}
	if authHeader != "Bearer my-api-key" {
		t.Fatalf("Authorization = %q, want 'Bearer my-api-key'", authHeader)
	}
}

func TestSearchPostsWithBearerAuth(t *testing.T) {
	var method string
	var authHeader string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		method = r.Method
		authHeader = r.Header.Get("Authorization")
		json.NewEncoder(w).Encode(FederatedResult{Results: []GeneResult{}, Meta: FederationMeta{}})
	}))
	defer srv.Close()

	seed := [32]byte{}
	c := NewClient(Config{BaseURL: srv.URL, APIKey: "key-123", Seed: seed, NodeID: "n1"})
	_, err := c.Search(context.Background(), &FederatedQuery{Query: "bugfix"})
	if err != nil {
		t.Fatalf("Search error: %v", err)
	}
	if method != "POST" {
		t.Fatalf("method = %s, want POST", method)
	}
	if authHeader != "Bearer key-123" {
		t.Fatalf("Authorization = %q, want 'Bearer key-123'", authHeader)
	}
}
