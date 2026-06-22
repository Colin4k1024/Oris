package evolution

import (
	"context"
	"fmt"
	"sort"
	"time"

	"github.com/Colin4k1024/Oris/sdks/go/store"
)

type Adapter struct {
	Store  store.Store
	Policy ReplayPolicy
}

func NewAdapter(s store.Store, policy ReplayPolicy) *Adapter {
	if policy.Mode == "" {
		policy.Mode = ReplayModeSuggest
	}
	if policy.Limit <= 0 {
		policy.Limit = 5
	}
	return &Adapter{Store: s, Policy: policy}
}

func (a *Adapter) Detect(_ context.Context, err error, taskClass string, meta map[string]any) EvolutionSignal {
	return DetectSignal(err, taskClass, meta)
}

func (a *Adapter) Select(ctx context.Context, signal EvolutionSignal) ([]EvolutionCandidate, error) {
	if a.Store == nil {
		return nil, fmt.Errorf("evolution adapter store is nil")
	}
	limit := a.Policy.Limit
	if limit <= 0 {
		limit = 5
	}
	genes, err := a.Store.Query(ctx, store.StoreQuery{
		TaskClass:     signal.TaskClass,
		MinConfidence: a.Policy.MinConfidence,
		OrderBy:       "confidence DESC",
		Limit:         limit * 3,
	})
	if err != nil {
		return nil, err
	}

	candidates := make([]EvolutionCandidate, 0, len(genes))
	for _, gene := range genes {
		if !matchesSignal(gene, signal) {
			continue
		}
		candidates = append(candidates, candidateFromGene(gene, signal))
	}
	sort.SliceStable(candidates, func(i, j int) bool {
		return candidates[i].Confidence > candidates[j].Confidence
	})
	if len(candidates) > limit {
		candidates = candidates[:limit]
	}
	return candidates, nil
}

func (a *Adapter) Replay(_ context.Context, candidate EvolutionCandidate) ReplayDecision {
	mode := a.Policy.Mode
	if mode == "" {
		mode = ReplayModeSuggest
	}
	if candidate.GeneID == "" || candidate.Confidence < a.Policy.MinConfidence {
		return ReplayDecision{Mode: ReplayModeSkip}
	}
	return ReplayDecision{
		Mode:         mode,
		Candidate:    &candidate,
		Instructions: candidate.Steps,
		Metadata: map[string]any{
			"rationale": candidate.Rationale,
		},
	}
}

func (a *Adapter) Solidify(ctx context.Context, input SolidifyInput) (store.Gene, error) {
	if a.Store == nil {
		return store.Gene{}, fmt.Errorf("evolution adapter store is nil")
	}
	if !input.Validation.Passed {
		return store.Gene{}, fmt.Errorf("validation did not pass")
	}
	now := time.Now().UTC()
	geneID := input.GeneID
	if geneID == "" {
		geneID = "evo-" + Fingerprint(input.Signal.Fingerprint, input.Solution, now.Format(time.RFC3339Nano))
	}
	name := input.Name
	if name == "" {
		name = "Evolution pattern for " + input.Signal.TaskClass
	}
	gene := store.Gene{
		GeneID:     geneID,
		Name:       name,
		TaskClass:  input.Signal.TaskClass,
		Confidence: confidenceFromValidation(input.Validation),
		Strategy: map[string]any{
			"solution": input.Solution,
			"steps":    input.Steps,
			"tags":     input.Tags,
		},
		Signals: map[string]any{
			"fingerprint": input.Signal.Fingerprint,
			"error_type":  input.Signal.ErrorType,
			"message":     input.Signal.Message,
		},
		Validation: map[string]any{
			"passed":   input.Validation.Passed,
			"evidence": input.Validation.Evidence,
			"metrics":  input.Validation.Metrics,
		},
		QualityScore:  0.8,
		SuccessCount:  1,
		ContributorID: "oris-adapter",
		Source:        "local",
		CreatedAt:     now,
		UpdatedAt:     now,
	}
	if err := a.Store.Save(ctx, gene); err != nil {
		return store.Gene{}, err
	}
	return gene, nil
}

func (a *Adapter) Status(ctx context.Context) (Status, error) {
	if a.Store == nil {
		return Status{}, fmt.Errorf("evolution adapter store is nil")
	}
	genes, err := a.Store.List(ctx, store.ListOpts{Limit: 1000})
	if err != nil {
		return Status{}, err
	}
	status := Status{GenesCount: len(genes)}
	for _, gene := range genes {
		status.AvgConfidence += gene.Confidence
		status.TotalUses += gene.UseCount
		status.SuccessfulUses += gene.SuccessCount
	}
	if status.GenesCount > 0 {
		status.AvgConfidence = status.AvgConfidence / float64(status.GenesCount)
	}
	if status.TotalUses > 0 {
		status.ReuseRate = float64(status.SuccessfulUses) / float64(status.TotalUses)
	}
	return status, nil
}

func matchesSignal(gene store.Gene, signal EvolutionSignal) bool {
	if gene.Signals == nil {
		return true
	}
	if fp, ok := gene.Signals["fingerprint"].(string); ok && fp != "" {
		return fp == signal.Fingerprint
	}
	if errorType, ok := gene.Signals["error_type"].(string); ok && errorType != "" {
		return errorType == signal.ErrorType
	}
	return true
}

func candidateFromGene(gene store.Gene, signal EvolutionSignal) EvolutionCandidate {
	return EvolutionCandidate{
		GeneID:     gene.GeneID,
		CapsuleID:  stringValue(gene.Strategy, "capsule_id"),
		Confidence: gene.Confidence,
		Rationale:  stringValue(gene.Strategy, "rationale"),
		Steps:      stepsValue(gene.Strategy),
		Metadata: map[string]any{
			"task_class":  gene.TaskClass,
			"fingerprint": signal.Fingerprint,
			"source":      gene.Source,
		},
	}
}

func stringValue(values map[string]any, key string) string {
	if values == nil {
		return ""
	}
	if value, ok := values[key].(string); ok {
		return value
	}
	return ""
}

func stepsValue(values map[string]any) []string {
	if values == nil {
		return nil
	}
	raw, ok := values["steps"]
	if !ok {
		if solution, ok := values["solution"].(string); ok && solution != "" {
			return []string{solution}
		}
		return nil
	}
	switch typed := raw.(type) {
	case []string:
		return typed
	case []any:
		steps := make([]string, 0, len(typed))
		for _, step := range typed {
			if s, ok := step.(string); ok && s != "" {
				steps = append(steps, s)
			}
		}
		return steps
	default:
		return nil
	}
}

func confidenceFromValidation(validation ValidationResult) float64 {
	if score, ok := validation.Metrics["confidence"].(float64); ok && score > 0 {
		return score
	}
	return 0.8
}
