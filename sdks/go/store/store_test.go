package store

import (
	"context"
	"fmt"
	"testing"
	"time"
)

func newTestStore(t *testing.T) *LocalStore {
	t.Helper()
	s, err := Open(":memory:")
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	t.Cleanup(func() { s.Close() })
	return s
}

func sampleGene(id string) Gene {
	now := time.Now().UTC().Truncate(time.Second)
	return Gene{
		GeneID:        id,
		Name:          "test-gene-" + id,
		TaskClass:     "bugfix",
		Confidence:    0.85,
		Strategy:      map[string]any{"approach": "retry"},
		Signals:       map[string]any{"error_rate": 0.1},
		Validation:    map[string]any{"tests_passed": true},
		QualityScore:  0.9,
		UseCount:      5,
		SuccessCount:  4,
		ContributorID: "node-1",
		Source:        "local",
		CreatedAt:     now,
		UpdatedAt:     now,
	}
}

func TestSaveAndGet(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()
	g := sampleGene("gene-1")

	if err := s.Save(ctx, g); err != nil {
		t.Fatalf("save: %v", err)
	}

	got, err := s.Get(ctx, "gene-1")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if got == nil {
		t.Fatal("expected gene, got nil")
	}
	if got.GeneID != "gene-1" {
		t.Errorf("gene_id = %q, want %q", got.GeneID, "gene-1")
	}
	if got.Confidence != 0.85 {
		t.Errorf("confidence = %f, want 0.85", got.Confidence)
	}
	if got.Strategy["approach"] != "retry" {
		t.Errorf("strategy.approach = %v, want retry", got.Strategy["approach"])
	}
}

func TestGetNotFound(t *testing.T) {
	s := newTestStore(t)
	got, err := s.Get(context.Background(), "nonexistent")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if got != nil {
		t.Fatalf("expected nil, got %+v", got)
	}
}

func TestSaveUpsert(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()
	g := sampleGene("gene-1")

	if err := s.Save(ctx, g); err != nil {
		t.Fatalf("save: %v", err)
	}

	g.Confidence = 0.95
	g.Name = "updated-gene"
	if err := s.Save(ctx, g); err != nil {
		t.Fatalf("upsert: %v", err)
	}

	got, err := s.Get(ctx, "gene-1")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if got.Confidence != 0.95 {
		t.Errorf("confidence = %f, want 0.95", got.Confidence)
	}
	if got.Name != "updated-gene" {
		t.Errorf("name = %q, want %q", got.Name, "updated-gene")
	}
}

func TestDelete(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	if err := s.Save(ctx, sampleGene("gene-1")); err != nil {
		t.Fatalf("save: %v", err)
	}
	if err := s.Delete(ctx, "gene-1"); err != nil {
		t.Fatalf("delete: %v", err)
	}

	got, err := s.Get(ctx, "gene-1")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if got != nil {
		t.Fatal("expected nil after delete")
	}
}

func TestQueryByTaskClass(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	g1 := sampleGene("gene-1")
	g1.TaskClass = "bugfix"
	g2 := sampleGene("gene-2")
	g2.TaskClass = "feature"

	s.Save(ctx, g1)
	s.Save(ctx, g2)

	results, err := s.Query(ctx, StoreQuery{TaskClass: "bugfix"})
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("len = %d, want 1", len(results))
	}
	if results[0].GeneID != "gene-1" {
		t.Errorf("gene_id = %q, want gene-1", results[0].GeneID)
	}
}

func TestQueryByMinConfidence(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	g1 := sampleGene("gene-low")
	g1.Confidence = 0.3
	g2 := sampleGene("gene-high")
	g2.Confidence = 0.9

	s.Save(ctx, g1)
	s.Save(ctx, g2)

	results, err := s.Query(ctx, StoreQuery{MinConfidence: 0.8})
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("len = %d, want 1", len(results))
	}
	if results[0].GeneID != "gene-high" {
		t.Errorf("gene_id = %q, want gene-high", results[0].GeneID)
	}
}

func TestQueryBySearchTerm(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	g1 := sampleGene("gene-1")
	g1.Name = "retry-handler"
	g2 := sampleGene("gene-2")
	g2.Name = "cache-warmer"

	s.Save(ctx, g1)
	s.Save(ctx, g2)

	results, err := s.Query(ctx, StoreQuery{Q: "retry"})
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("len = %d, want 1", len(results))
	}
	if results[0].Name != "retry-handler" {
		t.Errorf("name = %q, want retry-handler", results[0].Name)
	}
}

func TestQueryPagination(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	for i := 0; i < 5; i++ {
		g := sampleGene(fmt.Sprintf("gene-%d", i))
		g.CreatedAt = time.Now().UTC().Add(time.Duration(i) * time.Second)
		s.Save(ctx, g)
	}

	page1, err := s.Query(ctx, StoreQuery{Limit: 2, Offset: 0})
	if err != nil {
		t.Fatalf("query page1: %v", err)
	}
	if len(page1) != 2 {
		t.Fatalf("page1 len = %d, want 2", len(page1))
	}

	page2, err := s.Query(ctx, StoreQuery{Limit: 2, Offset: 2})
	if err != nil {
		t.Fatalf("query page2: %v", err)
	}
	if len(page2) != 2 {
		t.Fatalf("page2 len = %d, want 2", len(page2))
	}

	if page1[0].GeneID == page2[0].GeneID {
		t.Error("pagination returned duplicate results")
	}
}

func TestUpdateStats(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	g := sampleGene("gene-1")
	g.UseCount = 0
	g.SuccessCount = 0
	s.Save(ctx, g)

	if err := s.UpdateStats(ctx, "gene-1", true, true); err != nil {
		t.Fatalf("update stats (use+success): %v", err)
	}
	got, _ := s.Get(ctx, "gene-1")
	if got.UseCount != 1 || got.SuccessCount != 1 {
		t.Errorf("use=%d success=%d, want 1,1", got.UseCount, got.SuccessCount)
	}

	if err := s.UpdateStats(ctx, "gene-1", true, false); err != nil {
		t.Fatalf("update stats (use only): %v", err)
	}
	got, _ = s.Get(ctx, "gene-1")
	if got.UseCount != 2 || got.SuccessCount != 1 {
		t.Errorf("use=%d success=%d, want 2,1", got.UseCount, got.SuccessCount)
	}
}

func TestGetUnsyncedAndMarkSynced(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	g := sampleGene("gene-1")
	g.Source = "local"
	s.Save(ctx, g)

	unsynced, err := s.GetUnsynced(ctx)
	if err != nil {
		t.Fatalf("get unsynced: %v", err)
	}
	if len(unsynced) != 1 {
		t.Fatalf("unsynced len = %d, want 1", len(unsynced))
	}

	now := time.Now().UTC()
	if err := s.MarkSynced(ctx, "gene-1", now); err != nil {
		t.Fatalf("mark synced: %v", err)
	}

	got, _ := s.Get(ctx, "gene-1")
	if got.SyncedAt == nil {
		t.Fatal("synced_at should not be nil")
	}
}

func TestSyncLog(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	entry := SyncLogEntry{
		Direction:    "push",
		GeneID:       "gene-1",
		Status:       "success",
		RemoteURL:    "http://localhost:3000",
		ErrorMessage: "",
		Timestamp:    time.Now().UTC(),
	}
	if err := s.LogSync(ctx, entry); err != nil {
		t.Fatalf("log sync: %v", err)
	}

	logs, err := s.GetSyncLog(ctx, 10)
	if err != nil {
		t.Fatalf("get sync log: %v", err)
	}
	if len(logs) != 1 {
		t.Fatalf("logs len = %d, want 1", len(logs))
	}
	if logs[0].Direction != "push" {
		t.Errorf("direction = %q, want push", logs[0].Direction)
	}
	if logs[0].GeneID != "gene-1" {
		t.Errorf("gene_id = %q, want gene-1", logs[0].GeneID)
	}
}

func TestSyncedAtNullable(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()

	g := sampleGene("gene-1")
	g.SyncedAt = nil
	s.Save(ctx, g)

	got, _ := s.Get(ctx, "gene-1")
	if got.SyncedAt != nil {
		t.Error("synced_at should be nil for unsynced gene")
	}

	now := time.Now().UTC()
	g.SyncedAt = &now
	s.Save(ctx, g)

	got, _ = s.Get(ctx, "gene-1")
	if got.SyncedAt == nil {
		t.Error("synced_at should not be nil after setting")
	}
}
