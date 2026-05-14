package experience

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"time"

	"github.com/Colin4k1024/Oris/sdks/go/internal"
)

type Config struct {
	BaseURL    string
	APIKey     string
	Seed       [32]byte
	SenderID   string
	HTTPClient *http.Client
}

type Client struct {
	cfg  Config
	http *http.Client
}

func NewClient(cfg Config) *Client {
	hc := cfg.HTTPClient
	if hc == nil {
		hc = http.DefaultClient
	}
	return &Client{cfg: cfg, http: hc}
}

func (c *Client) Share(ctx context.Context, payload any) (*ShareResponse, error) {
	sig, err := internal.SignPayload(c.cfg.Seed, payload)
	if err != nil {
		return nil, fmt.Errorf("sign payload: %w", err)
	}

	envelope := OenEnvelope{
		SenderID:    c.cfg.SenderID,
		MessageType: "publish",
		Payload:     payload,
		Signature:   sig,
		Timestamp:   time.Now().UTC().Format(time.RFC3339),
	}

	body, err := json.Marshal(ShareRequest{Envelope: envelope})
	if err != nil {
		return nil, fmt.Errorf("marshal share request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.BaseURL+"/experience", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Api-Key", c.cfg.APIKey)

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("experience share: status %d: %s", resp.StatusCode, string(b))
	}

	var result ShareResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("decode share response: %w", err)
	}
	return &result, nil
}

func (c *Client) Fetch(ctx context.Context, query *FetchQuery) (*FetchResponse, error) {
	params := url.Values{}
	if query != nil {
		if query.Q != "" {
			params.Set("q", query.Q)
		}
		if query.MinConfidence > 0 {
			params.Set("min_confidence", strconv.FormatFloat(query.MinConfidence, 'f', -1, 64))
		}
		if query.Limit > 0 {
			params.Set("limit", strconv.Itoa(query.Limit))
		}
		if query.Cursor != "" {
			params.Set("cursor", query.Cursor)
		}
	}

	path := c.cfg.BaseURL + "/experience"
	if len(params) > 0 {
		path += "?" + params.Encode()
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, path, nil)
	if err != nil {
		return nil, err
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("experience fetch: status %d: %s", resp.StatusCode, string(b))
	}

	var result FetchResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("decode fetch response: %w", err)
	}
	return &result, nil
}

func (c *Client) RegisterPublicKey(ctx context.Context) error {
	body, err := json.Marshal(RegisterPublicKeyRequest{
		SenderID:     c.cfg.SenderID,
		PublicKeyHex: internal.PublicKeyHex(c.cfg.Seed),
	})
	if err != nil {
		return fmt.Errorf("marshal register key: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.BaseURL+"/public-keys", bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Api-Key", c.cfg.APIKey)

	resp, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("register public key: status %d: %s", resp.StatusCode, string(b))
	}
	return nil
}
