package sync

import (
	"context"
	"fmt"
	"time"

	"github.com/Colin4k1024/Oris/sdks/go/experience"
	"github.com/Colin4k1024/Oris/sdks/go/hub"
	"github.com/Colin4k1024/Oris/sdks/go/store"
)

type Config struct {
	Store      *store.LocalStore
	Experience *experience.Client
	Hub        *hub.Client
}

type SyncManager struct {
	store      *store.LocalStore
	experience *experience.Client
	hub        *hub.Client
}

func New(cfg Config) *SyncManager {
	return &SyncManager{
		store:      cfg.Store,
		experience: cfg.Experience,
		hub:        cfg.Hub,
	}
}

func (m *SyncManager) PushToHub(ctx context.Context, opts store.PushOpts) (*store.PushResult, error) {
	if m.experience == nil {
		return nil, fmt.Errorf("experience client not configured")
	}

	var genes []store.Gene
	var err error

	if len(opts.GeneIDs) > 0 {
		for _, id := range opts.GeneIDs {
			g, err := m.store.Get(ctx, id)
			if err != nil {
				return nil, fmt.Errorf("get gene %s: %w", id, err)
			}
			if g != nil {
				genes = append(genes, *g)
			}
		}
	} else {
		genes, err = m.store.GetUnsynced(ctx)
		if err != nil {
			return nil, fmt.Errorf("get unsynced: %w", err)
		}
	}

	result := &store.PushResult{}
	for _, g := range genes {
		payload := geneToPayload(g)
		_, shareErr := m.experience.Share(ctx, payload)

		entry := store.SyncLogEntry{
			Direction: "push",
			GeneID:    g.GeneID,
			Timestamp: time.Now().UTC(),
		}

		if shareErr != nil {
			entry.Status = "failed"
			entry.ErrorMessage = shareErr.Error()
			result.Failed++
			result.Errors = append(result.Errors, store.PushError{
				GeneID:  g.GeneID,
				Message: shareErr.Error(),
			})
		} else {
			entry.Status = "success"
			result.Pushed++
			m.store.MarkSynced(ctx, g.GeneID, time.Now().UTC())
		}
		m.store.LogSync(ctx, entry)
	}
	return result, nil
}

func (m *SyncManager) PullFromHub(ctx context.Context, opts store.PullOpts) (int, error) {
	if m.experience == nil {
		return 0, fmt.Errorf("experience client not configured")
	}

	query := &experience.FetchQuery{
		Q:             opts.Q,
		MinConfidence: opts.MinConfidence,
		Limit:         opts.Limit,
	}

	resp, err := m.experience.Fetch(ctx, query)
	if err != nil {
		return 0, fmt.Errorf("fetch from hub: %w", err)
	}

	imported := 0
	for _, asset := range resp.Assets {
		gene := assetToGene(asset)

		existing, err := m.store.Get(ctx, gene.GeneID)
		if err != nil {
			continue
		}

		if existing != nil {
			if coreFieldsMatch(existing, &gene) {
				gene = mergeStats(*existing, gene)
			} else {
				m.store.LogSync(ctx, store.SyncLogEntry{
					Direction:    "pull",
					GeneID:       gene.GeneID,
					Status:       "conflict",
					ErrorMessage: "core fields differ, skipped",
					Timestamp:    time.Now().UTC(),
				})
				continue
			}
		}

		if err := m.store.Save(ctx, gene); err != nil {
			continue
		}

		m.store.LogSync(ctx, store.SyncLogEntry{
			Direction: "pull",
			GeneID:    gene.GeneID,
			Status:    "success",
			Timestamp: time.Now().UTC(),
		})
		imported++
	}
	return imported, nil
}

func (m *SyncManager) RegisterNode(ctx context.Context, req *hub.RegisterRequest) (*hub.RegisterResponse, error) {
	if m.hub == nil {
		return nil, fmt.Errorf("hub client not configured")
	}
	return m.hub.Register(ctx, req)
}

func (m *SyncManager) GetSyncLog(ctx context.Context, limit int) ([]store.SyncLogEntry, error) {
	return m.store.GetSyncLog(ctx, limit)
}

func geneToPayload(g store.Gene) map[string]any {
	p := map[string]any{
		"type":          "gene",
		"id":            g.GeneID,
		"confidence":    g.Confidence,
		"quality_score": g.QualityScore,
		"use_count":     g.UseCount,
		"success_count": g.SuccessCount,
		"created_at":    g.CreatedAt.Format(time.RFC3339),
	}
	if g.Strategy != nil {
		p["strategy"] = g.Strategy
	}
	if g.Signals != nil {
		p["signals"] = g.Signals
	}
	if g.Validation != nil {
		p["validation"] = g.Validation
	}
	if g.ContributorID != "" {
		p["contributor_id"] = g.ContributorID
	}
	return p
}

func assetToGene(a experience.NetworkAsset) store.Gene {
	now := time.Now().UTC()
	g := store.Gene{
		GeneID:        a.ID,
		Name:          a.ID,
		TaskClass:     "unknown",
		Confidence:    a.Confidence,
		QualityScore:  a.QualityScore,
		UseCount:      a.UseCount,
		SuccessCount:  a.SuccessCount,
		ContributorID: a.ContributorID,
		Source:        "hub",
		CreatedAt:     a.CreatedAt,
		UpdatedAt:     now,
		SyncedAt:      &now,
	}

	if s, ok := a.Strategy.(map[string]any); ok {
		g.Strategy = s
	}
	if s, ok := a.Signals.(map[string]any); ok {
		g.Signals = s
	}
	if s, ok := a.Validation.(map[string]any); ok {
		g.Validation = s
	}

	return g
}

func coreFieldsMatch(existing, incoming *store.Gene) bool {
	return existing.TaskClass == incoming.TaskClass
}

func mergeStats(existing, incoming store.Gene) store.Gene {
	if incoming.UseCount > existing.UseCount {
		existing.UseCount = incoming.UseCount
	}
	if incoming.SuccessCount > existing.SuccessCount {
		existing.SuccessCount = incoming.SuccessCount
	}
	if incoming.Confidence > existing.Confidence {
		existing.Confidence = incoming.Confidence
	}
	if incoming.QualityScore > existing.QualityScore {
		existing.QualityScore = incoming.QualityScore
	}
	existing.UpdatedAt = time.Now().UTC()
	now := time.Now().UTC()
	existing.SyncedAt = &now
	return existing
}
