use std::fmt;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrPayload {
    pub issue_id: String,
    pub head: String,
    pub base: String,
    pub evidence_bundle_id: String,
    pub body: String,
}

impl PrPayload {
    pub fn new(
        issue_id: &str,
        head: &str,
        base: &str,
        evidence_bundle_id: &str,
        body: &str,
    ) -> Self {
        Self {
            issue_id: issue_id.to_string(),
            head: head.to_string(),
            base: base.to_string(),
            evidence_bundle_id: evidence_bundle_id.to_string(),
            body: body.to_string(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.evidence_bundle_id.trim().is_empty() {
            return Err("evidence_bundle_id is required");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedPullRequest {
    pub number: u64,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct GitHubAdapterError {
    message: String,
}

impl GitHubAdapterError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GitHubAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GitHubAdapterError {}

#[async_trait]
pub trait GitHubAdapter: Send + Sync {
    async fn create_pull_request(
        &self,
        payload: &PrPayload,
    ) -> Result<CreatedPullRequest, GitHubAdapterError>;
}

#[derive(Clone, Default)]
pub struct InMemoryGitHubAdapter {
    inner: Arc<Mutex<InMemoryGitHubState>>,
}

#[derive(Default)]
struct InMemoryGitHubState {
    next_pr_number: u64,
    payloads: Vec<PrPayload>,
}

impl InMemoryGitHubAdapter {
    pub fn recorded_payloads(&self) -> Vec<PrPayload> {
        self.inner
            .lock()
            .expect("in-memory github lock")
            .payloads
            .clone()
    }
}

#[async_trait]
impl GitHubAdapter for InMemoryGitHubAdapter {
    async fn create_pull_request(
        &self,
        payload: &PrPayload,
    ) -> Result<CreatedPullRequest, GitHubAdapterError> {
        payload
            .validate()
            .map_err(|e| GitHubAdapterError::new(e.to_string()))?;

        let mut state = self.inner.lock().expect("in-memory github lock");
        state.next_pr_number += 1;
        state.payloads.push(payload.clone());

        Ok(CreatedPullRequest {
            number: state.next_pr_number,
            url: format!("https://example.test/pr/{}", state.next_pr_number),
        })
    }
}
