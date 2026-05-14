package execution

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
)

type Config struct {
	BaseURL    string
	Token      string
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

func (c *Client) RunJob(ctx context.Context, req *RunJobRequest) (*RunJobResponse, error) {
	var resp RunJobResponse
	if err := c.postJSON(ctx, "/jobs", req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) GetState(ctx context.Context, threadID string) (*JobStateResponse, error) {
	var resp JobStateResponse
	if err := c.getJSON(ctx, fmt.Sprintf("/jobs/%s/state", threadID), &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) GetDetail(ctx context.Context, threadID string) (*JobDetailResponse, error) {
	var resp JobDetailResponse
	if err := c.getJSON(ctx, fmt.Sprintf("/jobs/%s", threadID), &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) GetHistory(ctx context.Context, threadID string) (*JobHistoryResponse, error) {
	var resp JobHistoryResponse
	if err := c.getJSON(ctx, fmt.Sprintf("/jobs/%s/history", threadID), &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) GetTimeline(ctx context.Context, threadID string) (*JobTimelineResponse, error) {
	var resp JobTimelineResponse
	if err := c.getJSON(ctx, fmt.Sprintf("/jobs/%s/timeline", threadID), &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Cancel(ctx context.Context, threadID string, reason string) (*CancelJobResponse, error) {
	req := CancelJobRequest{Reason: reason}
	var resp CancelJobResponse
	if err := c.postJSON(ctx, fmt.Sprintf("/jobs/%s/cancel", threadID), &req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Resume(ctx context.Context, threadID string, req *ResumeJobRequest) (*RunJobResponse, error) {
	var resp RunJobResponse
	if err := c.postJSON(ctx, fmt.Sprintf("/jobs/%s/resume", threadID), req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) Replay(ctx context.Context, threadID string, checkpointID string) (*RunJobResponse, error) {
	req := ReplayJobRequest{CheckpointID: checkpointID}
	var resp RunJobResponse
	if err := c.postJSON(ctx, fmt.Sprintf("/jobs/%s/replay", threadID), &req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) ListJobs(ctx context.Context, query *ListJobsQuery) (*ListJobsResponse, error) {
	params := url.Values{}
	if query != nil {
		if query.Status != "" {
			params.Set("status", query.Status)
		}
		if query.Limit > 0 {
			params.Set("limit", strconv.Itoa(query.Limit))
		}
		if query.Offset > 0 {
			params.Set("offset", strconv.Itoa(query.Offset))
		}
	}

	path := "/jobs"
	if len(params) > 0 {
		path += "?" + params.Encode()
	}

	var resp ListJobsResponse
	if err := c.getJSON(ctx, path, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (c *Client) postJSON(ctx context.Context, path string, body any, dest any) error {
	data, err := json.Marshal(body)
	if err != nil {
		return fmt.Errorf("marshal request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.BaseURL+path, bytes.NewReader(data))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.cfg.Token)

	return c.doEnvelope(req, dest)
}

func (c *Client) getJSON(ctx context.Context, path string, dest any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.cfg.BaseURL+path, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+c.cfg.Token)

	return c.doEnvelope(req, dest)
}

func (c *Client) doEnvelope(req *http.Request, dest any) error {
	resp, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("execution: status %d: %s", resp.StatusCode, string(body))
	}

	var envelope ApiEnvelope[json.RawMessage]
	if err := json.NewDecoder(resp.Body).Decode(&envelope); err != nil {
		return fmt.Errorf("decode envelope: %w", err)
	}
	return json.Unmarshal(envelope.Data, dest)
}
