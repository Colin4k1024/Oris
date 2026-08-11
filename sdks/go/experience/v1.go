package experience

import "fmt"

// ExperienceBundleV1 mirrors spec/experience/experience-bundle-v1.schema.json.
// Strings preserve timestamp values during cross-language round trips.
type ExperienceBundleV1 struct {
	SchemaVersion string           `json:"schema_version"`
	Gene          GeneV1           `json:"gene"`
	Capsules      []CapsuleV1      `json:"capsules"`
	UsageReceipts []UsageReceiptV1 `json:"usage_receipts"`
}

type GeneV1 struct {
	ID               string           `json:"id"`
	Version          uint32           `json:"version"`
	Name             string           `json:"name"`
	Description      string           `json:"description"`
	Scope            string           `json:"scope"`
	TaskCategory     string           `json:"task_category"`
	Applicability    ApplicabilityV1  `json:"applicability"`
	Steps            []map[string]any `json:"steps"`
	ToolRequirements []map[string]any `json:"tool_requirements"`
	Safety           map[string]any   `json:"safety"`
	Validation       map[string]any   `json:"validation"`
	Provenance       map[string]any   `json:"provenance"`
	Lifecycle        string           `json:"lifecycle"`
	CreatedAt        string           `json:"created_at"`
	UpdatedAt        string           `json:"updated_at"`
	Metadata         map[string]any   `json:"metadata"`
}

type ApplicabilityV1 struct {
	RequiredSignals []string         `json:"required_signals"`
	ExcludedSignals []string         `json:"excluded_signals"`
	Environments    []map[string]any `json:"environments"`
	ProjectIDs      []string         `json:"project_ids"`
	TenantIDs       []string         `json:"tenant_ids"`
	DoNotUseWhen    []string         `json:"do_not_use_when"`
}

type CapsuleV1 struct {
	ID                     string         `json:"id"`
	GeneID                 string         `json:"gene_id"`
	GeneVersion            uint32         `json:"gene_version"`
	EnvironmentFingerprint string         `json:"environment_fingerprint"`
	TaskContextHash        string         `json:"task_context_hash"`
	ExecutionEvidenceHash  string         `json:"execution_evidence_hash"`
	Validation             map[string]any `json:"validation"`
	ArtifactRefs           []string       `json:"artifact_refs"`
	Redaction              string         `json:"redaction"`
	CreatedAt              string         `json:"created_at"`
}

type UsageReceiptV1 struct {
	ID               string         `json:"id"`
	GeneID           string         `json:"gene_id"`
	GeneVersion      uint32         `json:"gene_version"`
	AgentID          string         `json:"agent_id"`
	RunID            string         `json:"run_id"`
	TaskContextHash  string         `json:"task_context_hash"`
	Adoption         string         `json:"adoption"`
	AppliedStepIDs   []string       `json:"applied_step_ids"`
	Outcome          string         `json:"outcome"`
	FailureReason    *string        `json:"failure_reason,omitempty"`
	TestEvidenceRefs []string       `json:"test_evidence_refs"`
	Cost             map[string]any `json:"cost,omitempty"`
	CreatedAt        string         `json:"created_at"`
}

func (bundle ExperienceBundleV1) Validate() error {
	if bundle.SchemaVersion != "oris.experience.bundle/v1" {
		return fmt.Errorf("unsupported schema_version %q", bundle.SchemaVersion)
	}
	if len(bundle.Gene.Steps) == 0 {
		return fmt.Errorf("Gene requires steps")
	}
	for _, receipt := range bundle.UsageReceipts {
		if receipt.Adoption == "adopted" && receipt.Outcome == "succeeded" && len(receipt.TestEvidenceRefs) == 0 {
			return fmt.Errorf("successful adoption requires evidence")
		}
	}
	return nil
}
