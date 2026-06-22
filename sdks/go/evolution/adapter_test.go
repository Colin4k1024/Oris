package evolution

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/Colin4k1024/Oris/sdks/go/store"
)

func TestSignalFingerprintIsStable(t *testing.T) {
	a := NewSignal("compile", "error", "/tmp/a/main.go:12: undefined: Foo", nil)
	b := NewSignal("compile", "error", "/Users/me/project/main.go:99: undefined: Foo", nil)
	if a.Fingerprint != b.Fingerprint {
		t.Fatalf("fingerprint mismatch: %s != %s", a.Fingerprint, b.Fingerprint)
	}
}

func TestAdapterSelectReplaySolidify(t *testing.T) {
	ctx := context.Background()
	s, err := store.Open(filepath.Join(t.TempDir(), "genes.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()

	signal := NewSignal("compile", "error", "undefined: Foo", nil)
	now := time.Now().UTC()
	if err := s.Save(ctx, store.Gene{
		GeneID:     "gene-1",
		Name:       "Fix undefined symbol",
		TaskClass:  "compile",
		Confidence: 0.91,
		Strategy: map[string]any{
			"steps": []any{"import the missing package", "rerun the compiler"},
		},
		Signals: map[string]any{
			"fingerprint": signal.Fingerprint,
			"error_type":  signal.ErrorType,
		},
		Source:    "local",
		CreatedAt: now,
		UpdatedAt: now,
	}); err != nil {
		t.Fatal(err)
	}

	adapter := NewAdapter(s, ReplayPolicy{MinConfidence: 0.5})
	candidates, err := adapter.Select(ctx, signal)
	if err != nil {
		t.Fatal(err)
	}
	if len(candidates) != 1 {
		t.Fatalf("expected one candidate, got %d", len(candidates))
	}
	decision := adapter.Replay(ctx, candidates[0])
	if decision.Mode != ReplayModeSuggest {
		t.Fatalf("expected suggest decision, got %s", decision.Mode)
	}
	if len(decision.Instructions) != 2 {
		t.Fatalf("expected replay instructions, got %#v", decision.Instructions)
	}

	newGene, err := adapter.Solidify(ctx, SolidifyInput{
		Signal:   adapter.Detect(ctx, errors.New("undefined: Bar"), "compile", nil),
		Solution: "add missing declaration",
		Validation: ValidationResult{
			Passed:   true,
			Evidence: "go test ./... passed",
		},
		Steps: []string{"add missing declaration", "rerun tests"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if newGene.GeneID == "" {
		t.Fatal("expected generated gene id")
	}
	status, err := adapter.Status(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if status.GenesCount != 2 {
		t.Fatalf("expected two genes, got %d", status.GenesCount)
	}
}
