package hub

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"

	"github.com/Colin4k1024/Oris/sdks/go/internal"
)

type Config struct {
	BaseURL    string
	APIKey     string
	Seed       [32]byte
	NodeID     string
	HTTPClient *http.Client
}

type Client struct {
	cfg Config
	http *http.Client
}

func NewClient(cfg Config) *Client {
	hc := cfg.HTTPClient
	if hc == nil {
		hc = http.DefaultClient
	}
	return &Client{cfg: cfg, http: hc}
}

func (c *Client) Register(ctx context.Context, req *RegisterRequest) (*RegisterResponse, error) {
	req.PublicKey = internal.PublicKeyBase64(c.cfg.Seed)
	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("marshal register request: %w", err)
	}
	sig := internal.SignBody(c.cfg.Seed, body)

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.BaseURL+"/hub/nodes", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("X-OEN-Signature", sig)

	var resp RegisterResponse
	if err := c.doJSON(httpReq, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Heartbeat(ctx context.Context, status NodeStatus) (*HeartbeatResponse, error) {
	req := HeartbeatRequest{NodeID: c.cfg.NodeID, Status: status}
	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("marshal heartbeat request: %w", err)
	}
	sig := internal.SignBody(c.cfg.Seed, body)

	url := fmt.Sprintf("%s/hub/nodes/%s/heartbeat", c.cfg.BaseURL, c.cfg.NodeID)
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPut, url, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("X-OEN-Signature", sig)

	var resp HeartbeatResponse
	if err := c.doJSON(httpReq, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Discover(ctx context.Context, query *DiscoveryQuery) (*DiscoveryResult, error) {
	body, err := json.Marshal(query)
	if err != nil {
		return nil, fmt.Errorf("marshal discovery query: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, c.cfg.BaseURL+"/hub/nodes", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Authorization", "Bearer "+c.cfg.APIKey)

	var resp DiscoveryResult
	if err := c.doJSON(httpReq, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Search(ctx context.Context, query *FederatedQuery) (*FederatedResult, error) {
	body, err := json.Marshal(query)
	if err != nil {
		return nil, fmt.Errorf("marshal federated query: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.BaseURL+"/hub/search", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Authorization", "Bearer "+c.cfg.APIKey)

	var resp FederatedResult
	if err := c.doJSON(httpReq, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Subscribe(ctx context.Context, req *CreateSubscriptionRequest) (*Subscription, error) {
	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("marshal subscription request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.BaseURL+"/hub/subscriptions", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Authorization", "Bearer "+c.cfg.APIKey)

	var resp Subscription
	if err := c.doJSON(httpReq, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Unsubscribe(ctx context.Context, subID string) error {
	url := fmt.Sprintf("%s/hub/subscriptions/%s", c.cfg.BaseURL, subID)
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodDelete, url, nil)
	if err != nil {
		return err
	}
	httpReq.Header.Set("Authorization", "Bearer "+c.cfg.APIKey)

	resp, err := c.http.Do(httpReq)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("unsubscribe: unexpected status %d", resp.StatusCode)
	}
	return nil
}

func (c *Client) ListSubscriptions(ctx context.Context) ([]Subscription, error) {
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, c.cfg.BaseURL+"/hub/subscriptions", nil)
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("Authorization", "Bearer "+c.cfg.APIKey)

	var resp []Subscription
	if err := c.doJSON(httpReq, &resp); err != nil {
		return nil, err
	}
	return resp, nil
}

func (c *Client) doJSON(req *http.Request, dest interface{}) error {
	resp, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("hub: status %d: %s", resp.StatusCode, string(body))
	}
	return json.NewDecoder(resp.Body).Decode(dest)
}
