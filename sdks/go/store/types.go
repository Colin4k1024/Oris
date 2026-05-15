package store

import "time"

type Gene struct {
	GeneID        string         `json:"gene_id"`
	Name          string         `json:"name"`
	TaskClass     string         `json:"task_class"`
	Confidence    float64        `json:"confidence"`
	Strategy      map[string]any `json:"strategy,omitempty"`
	Signals       map[string]any `json:"signals,omitempty"`
	Validation    map[string]any `json:"validation,omitempty"`
	QualityScore  float64        `json:"quality_score,omitempty"`
	UseCount      int            `json:"use_count"`
	SuccessCount  int            `json:"success_count"`
	ContributorID string         `json:"contributor_id,omitempty"`
	Source        string         `json:"source"`
	SyncedAt      *time.Time     `json:"synced_at,omitempty"`
	CreatedAt     time.Time      `json:"created_at"`
	UpdatedAt     time.Time      `json:"updated_at"`
}

type StoreQuery struct {
	Q             string
	TaskClass     string
	MinConfidence float64
	Source        string
	OrderBy       string
	Limit         int
	Offset        int
}

type ListOpts struct {
	Limit  int
	Offset int
}

type PushOpts struct {
	GeneIDs []string
}

type PullOpts struct {
	Q             string
	MinConfidence float64
	Limit         int
	TaskClass     string
}

type PushResult struct {
	Pushed int
	Failed int
	Errors []PushError
}

type PushError struct {
	GeneID  string
	Message string
}

type SyncLogEntry struct {
	ID           int
	Direction    string
	GeneID       string
	Status       string
	RemoteURL    string
	ErrorMessage string
	Timestamp    time.Time
}
