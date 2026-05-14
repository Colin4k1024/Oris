package execution

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func envelopeResponse(data any) []byte {
	b, _ := json.Marshal(ApiEnvelope[any]{
		Meta:      ApiMeta{Status: "ok", APIVersion: "v1"},
		RequestID: "req-001",
		Data:      data,
	})
	return b
}

func TestRunJobSendsBearer(t *testing.T) {
	var authHeader string
	var receivedBody map[string]any

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		authHeader = r.Header.Get("Authorization")
		json.NewDecoder(r.Body).Decode(&receivedBody)
		w.Write(envelopeResponse(RunJobResponse{
			ThreadID: "t1", Status: "running", Interrupts: []any{}, IdempotentReplay: false,
		}))
	}))
	defer srv.Close()

	c := NewClient(Config{BaseURL: srv.URL, Token: "my-token"})
	resp, err := c.RunJob(context.Background(), &RunJobRequest{
		ThreadID: "t1",
		Input:    map[string]any{"task": "test"},
	})
	if err != nil {
		t.Fatalf("RunJob error: %v", err)
	}
	if authHeader != "Bearer my-token" {
		t.Fatalf("Authorization = %q, want 'Bearer my-token'", authHeader)
	}
	if resp.ThreadID != "t1" {
		t.Fatalf("ThreadID = %q, want 't1'", resp.ThreadID)
	}
}

func TestGetStateUnwrapsEnvelope(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/jobs/t1/state" {
			t.Fatalf("path = %s, want /jobs/t1/state", r.URL.Path)
		}
		w.Write(envelopeResponse(JobStateResponse{
			ThreadID: "t1", Values: map[string]any{"result": "done"},
		}))
	}))
	defer srv.Close()

	c := NewClient(Config{BaseURL: srv.URL, Token: "tok"})
	resp, err := c.GetState(context.Background(), "t1")
	if err != nil {
		t.Fatalf("GetState error: %v", err)
	}
	if resp.Values["result"] != "done" {
		t.Fatalf("Values[result] = %v, want 'done'", resp.Values["result"])
	}
}

func TestCancelSendsReason(t *testing.T) {
	var receivedBody map[string]any

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		json.NewDecoder(r.Body).Decode(&receivedBody)
		w.Write(envelopeResponse(CancelJobResponse{ThreadID: "t1", Status: "cancelled", Reason: "test"}))
	}))
	defer srv.Close()

	c := NewClient(Config{BaseURL: srv.URL, Token: "tok"})
	_, err := c.Cancel(context.Background(), "t1", "user abort")
	if err != nil {
		t.Fatalf("Cancel error: %v", err)
	}
	if receivedBody["reason"] != "user abort" {
		t.Fatalf("reason = %v, want 'user abort'", receivedBody["reason"])
	}
}
