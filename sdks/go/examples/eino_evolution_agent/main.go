package main

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"time"

	"github.com/Colin4k1024/Oris/sdks/go/einoadapter"
	"github.com/Colin4k1024/Oris/sdks/go/evolution"
	"github.com/Colin4k1024/Oris/sdks/go/store"
	"github.com/cloudwego/eino/compose"
)

func main() {
	ctx := context.Background()
	dbPath := filepath.Join(".", "eino_evolution_demo.db")
	localStore, err := store.Open(dbPath)
	if err != nil {
		panic(err)
	}
	defer localStore.Close()

	signal := evolution.NewSignal("eino-tool", "error", "tool failed: missing city parameter", nil)
	now := time.Now().UTC()
	if err := localStore.Save(ctx, store.Gene{
		GeneID:     "gene-eino-missing-city",
		Name:       "Recover missing city parameter",
		TaskClass:  "eino-tool",
		Confidence: 0.93,
		Strategy: map[string]any{
			"rationale": "The weather tool fails when the city field is absent.",
			"steps": []any{
				"inspect the tool arguments",
				"ask the model to provide a city value",
				"retry the tool call with the completed argument",
			},
		},
		Signals: map[string]any{
			"fingerprint": signal.Fingerprint,
			"error_type":  signal.ErrorType,
		},
		Source:    "local",
		CreatedAt: now,
		UpdatedAt: now,
	}); err != nil {
		panic(err)
	}

	adapter := evolution.NewAdapter(localStore, evolution.ReplayPolicy{
		MinConfidence: 0.7,
		Mode:          evolution.ReplayModeSuggest,
	})

	toolErr := errors.New("tool failed: missing city parameter")
	detected := adapter.Detect(ctx, toolErr, "eino-tool", map[string]any{
		"framework": "eino",
		"tool":      "get_weather",
	})

	candidates, err := adapter.Select(ctx, detected)
	if err != nil {
		panic(err)
	}
	if len(candidates) == 0 {
		fmt.Println("no reusable Oris experience found")
		return
	}

	decision := adapter.Replay(ctx, candidates[0])
	fmt.Printf("replay mode: %s\n", decision.Mode)
	for i, step := range decision.Instructions {
		fmt.Printf("%d. %s\n", i+1, step)
	}

	middleware := einoadapter.InvokableToolMiddleware(adapter, einoadapter.Config{TaskClass: "eino-tool"})
	wrappedTool := middleware(func(context.Context, *compose.ToolInput) (*compose.ToolOutput, error) {
		return nil, errors.New("tool failed: missing city parameter")
	})
	toolOutput, err := wrappedTool(ctx, &compose.ToolInput{Name: "get_weather", Arguments: `{}`, CallID: "call-1"})
	if err != nil {
		panic(err)
	}
	fmt.Println(toolOutput.Result)

	_, err = adapter.Solidify(ctx, evolution.SolidifyInput{
		Signal:   detected,
		Solution: "retry get_weather after filling the city argument",
		Validation: evolution.ValidationResult{
			Passed:   true,
			Evidence: "demo retry succeeded",
		},
		Steps: []string{
			"fill missing city argument",
			"retry get_weather",
		},
	})
	if err != nil {
		panic(err)
	}

	status, err := adapter.Status(ctx)
	if err != nil {
		panic(err)
	}
	fmt.Printf("genes: %d avg_confidence: %.2f\n", status.GenesCount, status.AvgConfidence)
}
