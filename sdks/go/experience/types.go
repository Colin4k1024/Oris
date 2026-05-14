package experience

import "time"

type OenEnvelope struct {
	SenderID    string `json:"sender_id"`
	MessageType string `json:"message_type"`
	Payload     any    `json:"payload"`
	Signature   string `json:"signature"`
	Timestamp   string `json:"timestamp"`
}

type ShareRequest struct {
	Envelope OenEnvelope `json:"envelope"`
}

type ShareResponse struct {
	GeneID      string    `json:"gene_id"`
	Status      string    `json:"status"`
	PublishedAt time.Time `json:"published_at"`
}

type FetchQuery struct {
	Q             string  `json:"q,omitempty"`
	MinConfidence float64 `json:"min_confidence,omitempty"`
	Limit         int     `json:"limit,omitempty"`
	Cursor        string  `json:"cursor,omitempty"`
}

type FetchResponse struct {
	Assets     []NetworkAsset `json:"assets"`
	NextCursor *string        `json:"next_cursor"`
	SyncAudit  SyncAudit      `json:"sync_audit"`
}

type SyncAudit struct {
	TotalAvailable int `json:"total_available"`
	Returned       int `json:"returned"`
}

type NetworkAsset struct {
	Type          string    `json:"type"`
	ID            string    `json:"id"`
	Signals       any       `json:"signals,omitempty"`
	Strategy      any       `json:"strategy,omitempty"`
	Validation    any       `json:"validation,omitempty"`
	Confidence    float64   `json:"confidence,omitempty"`
	QualityScore  float64   `json:"quality_score,omitempty"`
	UseCount      int       `json:"use_count,omitempty"`
	SuccessCount  int       `json:"success_count,omitempty"`
	CreatedAt     time.Time `json:"created_at,omitempty"`
	ContributorID string    `json:"contributor_id,omitempty"`
}

type RegisterPublicKeyRequest struct {
	SenderID     string `json:"sender_id"`
	PublicKeyHex string `json:"public_key_hex"`
}
