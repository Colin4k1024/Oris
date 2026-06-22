package einoadapter

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/Colin4k1024/Oris/sdks/go/evolution"
	"github.com/Colin4k1024/Oris/sdks/go/store"
	"github.com/cloudwego/eino/compose"
)

func TestInvokableToolMiddlewareReturnsReplayMessageOnKnownFailure(t *testing.T) {
	ctx := context.Background()
	s, err := store.Open(filepath.Join(t.TempDir(), "genes.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()

	signal := evolution.NewSignal("eino-tool", "error", "missing city parameter", nil)
	now := time.Now().UTC()
	err = s.Save(ctx, store.Gene{
		GeneID:     "gene-1",
		Name:       "Recover missing city",
		TaskClass:  "eino-tool",
		Confidence: 0.9,
		Strategy: map[string]any{
			"steps": []any{"ask for city", "retry tool"},
		},
		Signals: map[string]any{
			"fingerprint": signal.Fingerprint,
			"error_type":  signal.ErrorType,
		},
		Source:    "local",
		CreatedAt: now,
		UpdatedAt: now,
	})
	if err != nil {
		t.Fatal(err)
	}

	adapter := evolution.NewAdapter(s, evolution.ReplayPolicy{MinConfidence: 0.5})
	middleware := InvokableToolMiddleware(adapter, Config{TaskClass: "eino-tool"})
	endpoint := middleware(func(context.Context, *compose.ToolInput) (*compose.ToolOutput, error) {
		return nil, errors.New("missing city parameter")
	})

	output, err := endpoint(ctx, &compose.ToolInput{Name: "get_weather", Arguments: `{}`, CallID: "call-1"})
	if err != nil {
		t.Fatal(err)
	}
	if output == nil || !strings.Contains(output.Result, "ask for city") {
		t.Fatalf("expected replay output, got %#v", output)
	}
}

func TestInvokableToolMiddlewarePreservesUnknownFailure(t *testing.T) {
	ctx := context.Background()
	s, err := store.Open(filepath.Join(t.TempDir(), "genes.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()

	adapter := evolution.NewAdapter(s, evolution.ReplayPolicy{MinConfidence: 0.5})
	middleware := InvokableToolMiddleware(adapter, Config{TaskClass: "eino-tool"})
	originalErr := errors.New("unknown failure")
	endpoint := middleware(func(context.Context, *compose.ToolInput) (*compose.ToolOutput, error) {
		return nil, originalErr
	})

	output, err := endpoint(ctx, &compose.ToolInput{Name: "get_weather"})
	if !errors.Is(err, originalErr) {
		t.Fatalf("expected original error, got output=%#v err=%v", output, err)
	}
}
