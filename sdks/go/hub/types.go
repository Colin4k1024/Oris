package hub

import "time"

type RegisterRequest struct {
	NodeID       string   `json:"node_id"`
	Endpoint     string   `json:"endpoint"`
	PublicKey    string   `json:"public_key"`
	Capabilities []string `json:"capabilities"`
	Region       string   `json:"region,omitempty"`
	Version      string   `json:"version"`
}

type RegisterResponse struct {
	NodeID                  string `json:"node_id"`
	HeartbeatIntervalSeconds uint64 `json:"heartbeat_interval_seconds"`
	TTLSeconds              uint64 `json:"ttl_seconds"`
}

type HeartbeatRequest struct {
	NodeID string     `json:"node_id"`
	Status NodeStatus `json:"status,omitempty"`
}

type HeartbeatResponse struct {
	Acknowledged         bool   `json:"acknowledged"`
	NextHeartbeatSeconds uint64 `json:"next_heartbeat_seconds"`
}

type NodeStatus string

const (
	NodeStatusActive   NodeStatus = "active"
	NodeStatusDegraded NodeStatus = "degraded"
	NodeStatusOffline  NodeStatus = "offline"
)

type NodeInfo struct {
	NodeID        string     `json:"node_id"`
	Endpoint      string     `json:"endpoint"`
	PublicKey     string     `json:"public_key"`
	Capabilities  []string   `json:"capabilities"`
	Region        string     `json:"region,omitempty"`
	Version       string     `json:"version"`
	Status        NodeStatus `json:"status"`
	RegisteredAt  time.Time  `json:"registered_at"`
	LastHeartbeat time.Time  `json:"last_heartbeat"`
	TTLSeconds    uint64     `json:"ttl_seconds"`
}

type DiscoveryQuery struct {
	Capabilities []string `json:"capabilities,omitempty"`
	Region       string   `json:"region,omitempty"`
	Version      string   `json:"version,omitempty"`
	Limit        int      `json:"limit,omitempty"`
}

type DiscoveryResult struct {
	Nodes []NodeInfo `json:"nodes"`
	Total int        `json:"total"`
}

type FederatedQuery struct {
	Query         string   `json:"query"`
	TaskClass     string   `json:"task_class,omitempty"`
	MinConfidence float64  `json:"min_confidence,omitempty"`
	TimeoutMs     uint64   `json:"timeout_ms,omitempty"`
	TargetNodes   []string `json:"target_nodes,omitempty"`
	Limit         int      `json:"limit,omitempty"`
}

type FederatedResult struct {
	Results []GeneResult   `json:"results"`
	Meta    FederationMeta `json:"meta"`
}

type GeneResult struct {
	GeneID     string    `json:"gene_id"`
	Name       string    `json:"name"`
	TaskClass  string    `json:"task_class"`
	Confidence float64   `json:"confidence"`
	SourceNode string    `json:"source_node"`
	CreatedAt  time.Time `json:"created_at"`
}

type FederationMeta struct {
	NodesQueried   int       `json:"nodes_queried"`
	NodesResponded int       `json:"nodes_responded"`
	Coverage       float64   `json:"coverage"`
	Freshness      time.Time `json:"freshness"`
	TimeoutNodes   []string  `json:"timeout_nodes"`
}

type CreateSubscriptionRequest struct {
	SubscriberNodeID string             `json:"subscriber_node_id"`
	CallbackURL      string             `json:"callback_url"`
	Filter           SubscriptionFilter `json:"filter"`
}

type SubscriptionFilter struct {
	TaskClass     string   `json:"task_class,omitempty"`
	MinConfidence float64  `json:"min_confidence,omitempty"`
	SourceNodes   []string `json:"source_nodes,omitempty"`
}

type Subscription struct {
	ID               string             `json:"id"`
	SubscriberNodeID string             `json:"subscriber_node_id"`
	CallbackURL      string             `json:"callback_url"`
	Filter           SubscriptionFilter `json:"filter"`
	CreatedAt        time.Time          `json:"created_at"`
	Active           bool               `json:"active"`
}

type GenePromotedEvent struct {
	GeneID       string    `json:"gene_id"`
	GeneName     string    `json:"gene_name"`
	TaskClass    string    `json:"task_class"`
	Confidence   float64   `json:"confidence"`
	SourceNodeID string    `json:"source_node_id"`
	PromotedAt   time.Time `json:"promoted_at"`
}
