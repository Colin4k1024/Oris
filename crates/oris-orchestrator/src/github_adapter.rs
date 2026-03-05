use std::fmt;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

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

impl From<reqwest::Error> for GitHubAdapterError {
    fn from(value: reqwest::Error) -> Self {
        Self::new(format!("github http error: {}", value))
    }
}

#[async_trait]
pub trait GitHubAdapter: Send + Sync {
    async fn create_pull_request(
        &self,
        payload: &PrPayload,
    ) -> Result<CreatedPullRequest, GitHubAdapterError>;
}

#[derive(Clone)]
pub struct GitHubApiAdapter {
    owner: String,
    repo: String,
    token: String,
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct CreatePullRequestRequest {
    title: String,
    head: String,
    base: String,
    body: String,
}

#[derive(Debug, Deserialize)]
struct CreatePullRequestResponse {
    number: u64,
    html_url: String,
}

impl GitHubApiAdapter {
    pub fn new(
        owner: impl Into<String>,
        repo: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
            token: token.into(),
            base_url: "https://api.github.com".to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(
        owner: impl Into<String>,
        repo: impl Into<String>,
        token: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
            token: token.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    fn issue_title(issue_id: &str) -> String {
        format!("Automated update for {}", issue_id)
    }
}

#[async_trait]
impl GitHubAdapter for GitHubApiAdapter {
    async fn create_pull_request(
        &self,
        payload: &PrPayload,
    ) -> Result<CreatedPullRequest, GitHubAdapterError> {
        payload
            .validate()
            .map_err(|e| GitHubAdapterError::new(e.to_string()))?;

        if self.token.trim().is_empty() {
            return Err(GitHubAdapterError::new(
                "github token is required for API adapter",
            ));
        }

        let request = CreatePullRequestRequest {
            title: Self::issue_title(&payload.issue_id),
            head: payload.head.clone(),
            base: payload.base.clone(),
            body: payload.body.clone(),
        };

        let url = format!("{}/repos/{}/{}/pulls", self.base_url, self.owner, self.repo);

        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token),
            )
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(reqwest::header::USER_AGENT, "oris-orchestrator")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GitHubAdapterError::new(format!(
                "github api returned {}: {}",
                status, body
            )));
        }

        let created = response.json::<CreatePullRequestResponse>().await?;
        Ok(CreatedPullRequest {
            number: created.number,
            url: created.html_url,
        })
    }
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
