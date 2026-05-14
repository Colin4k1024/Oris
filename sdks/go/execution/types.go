package execution

import "time"

type ApiEnvelope[T any] struct {
	Meta      ApiMeta `json:"meta"`
	RequestID string  `json:"request_id"`
	Data      T       `json:"data"`
}

type ApiMeta struct {
	Status     string `json:"status"`
	APIVersion string `json:"api_version"`
}

type RunJobRequest struct {
	ThreadID       string                 `json:"thread_id"`
	Input          map[string]any         `json:"input,omitempty"`
	IdempotencyKey string                 `json:"idempotency_key,omitempty"`
	TimeoutPolicy  *TimeoutPolicyRequest  `json:"timeout_policy,omitempty"`
	Priority       *int32                 `json:"priority,omitempty"`
	TenantID       string                 `json:"tenant_id,omitempty"`
}

type TimeoutPolicyRequest struct {
	TimeoutMs       int64  `json:"timeout_ms"`
	OnTimeoutStatus string `json:"on_timeout_status"`
}

type RunJobResponse struct {
	ThreadID       string `json:"thread_id"`
	Status         string `json:"status"`
	Interrupts     []any  `json:"interrupts"`
	IdempotencyKey string `json:"idempotency_key,omitempty"`
	IdempotentReplay bool `json:"idempotent_replay"`
	Trace          any    `json:"trace,omitempty"`
}

type JobStateResponse struct {
	ThreadID     string         `json:"thread_id"`
	CheckpointID string        `json:"checkpoint_id,omitempty"`
	CreatedAt    *time.Time     `json:"created_at,omitempty"`
	Values       map[string]any `json:"values"`
}

type JobDetailResponse struct {
	ThreadID         string            `json:"thread_id"`
	Status           string            `json:"status"`
	CheckpointID     string            `json:"checkpoint_id,omitempty"`
	Values           map[string]any    `json:"values"`
	History          []JobHistoryItem  `json:"history"`
	Timeline         []JobTimelineItem `json:"timeline"`
	PendingInterrupt any               `json:"pending_interrupt,omitempty"`
	Trace            any               `json:"trace,omitempty"`
	Observability    any               `json:"observability,omitempty"`
}

type JobHistoryItem struct {
	CheckpointID string     `json:"checkpoint_id,omitempty"`
	CreatedAt    *time.Time `json:"created_at,omitempty"`
}

type JobHistoryResponse struct {
	ThreadID string           `json:"thread_id"`
	History  []JobHistoryItem `json:"history"`
}

type JobTimelineItem struct {
	Seq          uint64     `json:"seq"`
	EventType    string     `json:"event_type"`
	CheckpointID string     `json:"checkpoint_id,omitempty"`
	CreatedAt    *time.Time `json:"created_at,omitempty"`
}

type JobTimelineResponse struct {
	ThreadID      string            `json:"thread_id"`
	Timeline      []JobTimelineItem `json:"timeline"`
	Observability any               `json:"observability,omitempty"`
	Trace         any               `json:"trace,omitempty"`
}

type CancelJobRequest struct {
	Reason string `json:"reason,omitempty"`
}

type CancelJobResponse struct {
	ThreadID string `json:"thread_id"`
	Status   string `json:"status"`
	Reason   string `json:"reason,omitempty"`
}

type ResumeJobRequest struct {
	Value        map[string]any `json:"value"`
	CheckpointID string         `json:"checkpoint_id,omitempty"`
}

type ReplayJobRequest struct {
	CheckpointID string `json:"checkpoint_id,omitempty"`
}

type ListJobsQuery struct {
	Status string `json:"status,omitempty"`
	Limit  int    `json:"limit,omitempty"`
	Offset int    `json:"offset,omitempty"`
}

type ListJobsResponse struct {
	Jobs []JobListItem `json:"jobs"`
}

type JobListItem struct {
	ThreadID  string    `json:"thread_id"`
	Status    string    `json:"status"`
	UpdatedAt time.Time `json:"updated_at"`
}
