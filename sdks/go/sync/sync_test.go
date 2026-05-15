package sync

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/Colin4k1024/Oris/sdks/go/experience"
	"github.com/Colin4k1024/Oris/sdks/go/store"
)

func newTestStore(t *testing.T) *store.LocalStore {
	t.Helper()
	s, err := store.Open(":memory:")
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	t.Cleanup(func() { s.Close() })
	return s
}

func TestPushToHub(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPost && r.URL.Path == "/experience" {
			json.NewEncoder(w).Encode(experience.ShareResponse{
				GeneID:      "gene-1",
				Status:      "published",
				PublishedAt: time.Now(),
			})
			return
		}
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	s := newTestStore(t)
	ctx := context.Background()

	now := time.Now().UTC()
	g := store.Gene{
		GeneID:    "gene-1",
		Name:      "test-gene",
		TaskClass: "bugfix",
		Confidence: 0.85,
		Source:    "local",
		CreatedAt: now,
		UpdatedAt: now,
	}
	s.Save(ctx, g)

	expClient := experience.NewClient(experience.Config{
		BaseURL:    server.URL,
		APIKey:     "test-key",
		SenderID:   "test-node",
		HTTPClient: server.Client(),
	})

	mgr := New(Config{Store: s, Experience: expClient})
	result, err := mgr.PushToHub(ctx, store.PushOpts{})
	if err != nil {
		t.Fatalf("push: %v", err)
	}
	if result.Pushed != 1 {
		t.Errorf("pushed = %d, want 1", result.Pushed)
	}
	if result.Failed != 0 {
		t.Errorf("failed = %d, want 0", result.Failed)
	}

	got, _ := s.Get(ctx, "gene-1")
	if got.SyncedAt == nil {
		t.Error("gene should be marked as synced")
	}

	logs, _ := s.GetSyncLog(ctx, 10)
	if len(logs) != 1 {
		t.Fatalf("sync log len = %d, want 1", len(logs))
	}
	if logs[0].Status != "success" {
		t.Errorf("log status = %q, want success", logs[0].Status)
	}
}

func TestPushToHubFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		w.Write([]byte("internal error"))
	}))
	defer server.Close()

	s := newTestStore(t)
	ctx := context.Background()

	now := time.Now().UTC()
	s.Save(ctx, store.Gene{
		GeneID:    "gene-1",
		Name:      "test",
		TaskClass: "bugfix",
		Confidence: 0.5,
		Source:    "local",
		CreatedAt: now,
		UpdatedAt: now,
	})

	expClient := experience.NewClient(experience.Config{
		BaseURL:    server.URL,
		APIKey:     "test-key",
		SenderID:   "test-node",
		HTTPClient: server.Client(),
	})

	mgr := New(Config{Store: s, Experience: expClient})
	result, err := mgr.PushToHub(ctx, store.PushOpts{})
	if err != nil {
		t.Fatalf("push: %v", err)
	}
	if result.Failed != 1 {
		t.Errorf("failed = %d, want 1", result.Failed)
	}
	if result.Pushed != 0 {
		t.Errorf("pushed = %d, want 0", result.Pushed)
	}
	if len(result.Errors) != 1 {
		t.Fatalf("errors len = %d, want 1", len(result.Errors))
	}
}

func TestPullFromHub(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/experience" {
			resp := experience.FetchResponse{
				Assets: []experience.NetworkAsset{
					{
						Type:         "gene",
						ID:           "remote-gene-1",
						Confidence:   0.9,
						QualityScore: 0.85,
						UseCount:     10,
						SuccessCount: 8,
						CreatedAt:    time.Now().UTC(),
					},
				},
				SyncAudit: experience.SyncAudit{TotalAvailable: 1, Returned: 1},
			}
			json.NewEncoder(w).Encode(resp)
			return
		}
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	s := newTestStore(t)
	ctx := context.Background()

	expClient := experience.NewClient(experience.Config{
		BaseURL:    server.URL,
		APIKey:     "test-key",
		SenderID:   "test-node",
		HTTPClient: server.Client(),
	})

	mgr := New(Config{Store: s, Experience: expClient})
	imported, err := mgr.PullFromHub(ctx, store.PullOpts{Limit: 10})
	if err != nil {
		t.Fatalf("pull: %v", err)
	}
	if imported != 1 {
		t.Errorf("imported = %d, want 1", imported)
	}

	got, _ := s.Get(ctx, "remote-gene-1")
	if got == nil {
		t.Fatal("expected imported gene")
	}
	if got.Source != "hub" {
		t.Errorf("source = %q, want hub", got.Source)
	}
	if got.Confidence != 0.9 {
		t.Errorf("confidence = %f, want 0.9", got.Confidence)
	}
}

func TestPullConflictSkipsWhenDifferent(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		resp := experience.FetchResponse{
			Assets: []experience.NetworkAsset{
				{
					Type:       "gene",
					ID:         "gene-1",
					Confidence: 0.9,
				},
			},
		}
		json.NewEncoder(w).Encode(resp)
	}))
	defer server.Close()

	s := newTestStore(t)
	ctx := context.Background()

	now := time.Now().UTC()
	s.Save(ctx, store.Gene{
		GeneID:    "gene-1",
		Name:      "existing",
		TaskClass: "feature",
		Confidence: 0.5,
		Source:    "local",
		CreatedAt: now,
		UpdatedAt: now,
	})

	expClient := experience.NewClient(experience.Config{
		BaseURL:    server.URL,
		SenderID:   "test",
		HTTPClient: server.Client(),
	})

	mgr := New(Config{Store: s, Experience: expClient})
	imported, _ := mgr.PullFromHub(ctx, store.PullOpts{})
	if imported != 0 {
		t.Errorf("imported = %d, want 0 (conflict should skip)", imported)
	}

	logs, _ := s.GetSyncLog(ctx, 10)
	found := false
	for _, l := range logs {
		if l.Status == "conflict" {
			found = true
		}
	}
	if !found {
		t.Error("expected conflict log entry")
	}
}

func TestNoExperienceClient(t *testing.T) {
	s := newTestStore(t)
	mgr := New(Config{Store: s})

	_, err := mgr.PushToHub(context.Background(), store.PushOpts{})
	if err == nil {
		t.Error("expected error for push without experience client")
	}

	_, err = mgr.PullFromHub(context.Background(), store.PullOpts{})
	if err == nil {
		t.Error("expected error for pull without experience client")
	}
}
