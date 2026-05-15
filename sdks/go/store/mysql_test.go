package store

import (
	"context"
	"os"
	"testing"
	"time"
)

func getMySQLDSN(t *testing.T) string {
	dsn := os.Getenv("MYSQL_DSN")
	if dsn == "" {
		t.Skip("MYSQL_DSN not set, skipping MySQL tests")
	}
	return dsn
}

func TestMySQLStore_OpenAndClose(t *testing.T) {
	dsn := getMySQLDSN(t)
	s, err := OpenMySQLFromDSN(dsn)
	if err != nil {
		t.Fatalf("OpenMySQLFromDSN: %v", err)
	}
	defer s.Close()
}

func TestMySQLStore_SaveAndGet(t *testing.T) {
	dsn := getMySQLDSN(t)
	s, err := OpenMySQLFromDSN(dsn)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer s.Close()

	ctx := context.Background()
	now := time.Now().UTC().Truncate(time.Millisecond)

	gene := Gene{
		GeneID:        "test-mysql-gene-1",
		Name:          "Test Gene",
		TaskClass:     "compile-fix",
		Confidence:    0.85,
		Strategy:      map[string]any{"approach": "retry"},
		Signals:       map[string]any{"error_type": "syntax"},
		Validation:    map[string]any{"tests_passed": true},
		QualityScore:  0.9,
		UseCount:      5,
		SuccessCount:  4,
		ContributorID: "node-1",
		Source:        "local",
		CreatedAt:     now,
		UpdatedAt:     now,
	}

	if err := s.Save(ctx, gene); err != nil {
		t.Fatalf("Save: %v", err)
	}

	got, err := s.Get(ctx, "test-mysql-gene-1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got == nil {
		t.Fatal("Get returned nil")
	}
	if got.Name != "Test Gene" {
		t.Errorf("Name = %q, want %q", got.Name, "Test Gene")
	}
	if got.Confidence != 0.85 {
		t.Errorf("Confidence = %v, want 0.85", got.Confidence)
	}

	// cleanup
	_ = s.Delete(ctx, "test-mysql-gene-1")
}

func TestMySQLStore_UpdateStats(t *testing.T) {
	dsn := getMySQLDSN(t)
	s, err := OpenMySQLFromDSN(dsn)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer s.Close()

	ctx := context.Background()
	now := time.Now().UTC().Truncate(time.Millisecond)

	gene := Gene{
		GeneID:    "test-mysql-stats-1",
		Name:      "Stats Gene",
		TaskClass: "test",
		Source:    "local",
		CreatedAt: now,
		UpdatedAt: now,
	}
	_ = s.Save(ctx, gene)

	if err := s.UpdateStats(ctx, "test-mysql-stats-1", true, true); err != nil {
		t.Fatalf("UpdateStats: %v", err)
	}

	got, _ := s.Get(ctx, "test-mysql-stats-1")
	if got.UseCount != 1 || got.SuccessCount != 1 {
		t.Errorf("UseCount=%d SuccessCount=%d, want 1,1", got.UseCount, got.SuccessCount)
	}

	_ = s.Delete(ctx, "test-mysql-stats-1")
}

func TestMySQLStore_Query(t *testing.T) {
	dsn := getMySQLDSN(t)
	s, err := OpenMySQLFromDSN(dsn)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer s.Close()

	ctx := context.Background()
	now := time.Now().UTC().Truncate(time.Millisecond)

	for i := 0; i < 3; i++ {
		g := Gene{
			GeneID:    "test-mysql-q-" + string(rune('a'+i)),
			Name:      "Query Gene",
			TaskClass: "query-test",
			Source:    "local",
			CreatedAt: now,
			UpdatedAt: now,
		}
		_ = s.Save(ctx, g)
	}

	results, err := s.Query(ctx, StoreQuery{TaskClass: "query-test", Limit: 10})
	if err != nil {
		t.Fatalf("Query: %v", err)
	}
	if len(results) < 3 {
		t.Errorf("Query returned %d results, want >= 3", len(results))
	}

	for i := 0; i < 3; i++ {
		_ = s.Delete(ctx, "test-mysql-q-"+string(rune('a'+i)))
	}
}

func TestMySQLStore_SyncLog(t *testing.T) {
	dsn := getMySQLDSN(t)
	s, err := OpenMySQLFromDSN(dsn)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer s.Close()

	ctx := context.Background()

	entry := SyncLogEntry{
		Direction:    "push",
		GeneID:       "test-sync-log-gene",
		Status:       "success",
		RemoteURL:    "https://hub.example.com",
		ErrorMessage: "",
		Timestamp:    time.Now().UTC().Truncate(time.Millisecond),
	}

	if err := s.LogSync(ctx, entry); err != nil {
		t.Fatalf("LogSync: %v", err)
	}

	logs, err := s.GetSyncLog(ctx, 10)
	if err != nil {
		t.Fatalf("GetSyncLog: %v", err)
	}
	if len(logs) == 0 {
		t.Error("GetSyncLog returned empty")
	}
}
