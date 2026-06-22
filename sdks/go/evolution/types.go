package evolution

import "time"

const (
	ReplayModeSkip    = "skip"
	ReplayModeSuggest = "suggest"
	ReplayModeForce   = "force"
)

type EvolutionSignal struct {
	TaskClass   string         `json:"task_class"`
	ErrorType   string         `json:"error_type"`
	Message     string         `json:"message"`
	Fingerprint string         `json:"fingerprint"`
	Context     map[string]any `json:"context,omitempty"`
}

type EvolutionCandidate struct {
	GeneID     string         `json:"gene_id"`
	CapsuleID  string         `json:"capsule_id,omitempty"`
	Confidence float64        `json:"confidence"`
	Rationale  string         `json:"rationale,omitempty"`
	Steps      []string       `json:"steps,omitempty"`
	Metadata   map[string]any `json:"metadata,omitempty"`
}

type ReplayDecision struct {
	Mode         string              `json:"mode"`
	Candidate    *EvolutionCandidate `json:"candidate,omitempty"`
	Instructions []string            `json:"instructions,omitempty"`
	Metadata     map[string]any      `json:"metadata,omitempty"`
}

type ValidationResult struct {
	Passed   bool           `json:"passed"`
	Evidence string         `json:"evidence,omitempty"`
	Metrics  map[string]any `json:"metrics,omitempty"`
	Error    string         `json:"error,omitempty"`
}

type SolidifyInput struct {
	Signal     EvolutionSignal  `json:"signal"`
	Solution   string           `json:"solution"`
	Validation ValidationResult `json:"validation"`
	Tags       []string         `json:"tags,omitempty"`
	Steps      []string         `json:"steps,omitempty"`
	GeneID     string           `json:"gene_id,omitempty"`
	Name       string           `json:"name,omitempty"`
}

type ReplayPolicy struct {
	MinConfidence float64
	Mode          string
	Limit         int
}

type Status struct {
	GenesCount     int     `json:"genes_count"`
	AvgConfidence  float64 `json:"avg_confidence"`
	ReuseRate      float64 `json:"reuse_rate"`
	SuccessfulUses int     `json:"successful_uses"`
	TotalUses      int     `json:"total_uses"`
}

type Clock interface {
	Now() time.Time
}
