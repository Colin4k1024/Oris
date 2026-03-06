use std::fmt;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

const GITHUB_ACCEPT_HEADER: &str = "application/vnd.github+json";
const GITHUB_USER_AGENT: &str = "oris-orchestrator";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteIssue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub labels: Vec<String>,
    pub milestone_number: Option<u64>,
    pub milestone_title: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueListQuery {
    pub state: String,
    pub per_page: usize,
    pub max_pages: usize,
}

impl IssueListQuery {
    pub fn open_only() -> Self {
        Self {
            state: "open".to_string(),
            per_page: 100,
            max_pages: 10,
        }
    }
}

impl Default for IssueListQuery {
    fn default() -> Self {
        Self::open_only()
    }
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
    async fn list_issues(
        &self,
        query: &IssueListQuery,
    ) -> Result<Vec<RemoteIssue>, GitHubAdapterError>;

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

#[derive(Debug, Deserialize)]
struct IssueListLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct IssueListMilestone {
    number: u64,
    title: String,
}

#[derive(Debug, Deserialize)]
struct IssueListPullRequestMarker {}

#[derive(Debug, Deserialize)]
struct IssueListResponseItem {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    labels: Vec<IssueListLabel>,
    milestone: Option<IssueListMilestone>,
    created_at: Option<String>,
    pull_request: Option<IssueListPullRequestMarker>,
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

    fn normalize_pagination(query: &IssueListQuery) -> (usize, usize) {
        let per_page = query.per_page.max(1).min(100);
        let max_pages = query.max_pages.max(1);
        (per_page, max_pages)
    }

    async fn fetch_issue_page(
        &self,
        state: &str,
        per_page: usize,
        page: usize,
        include_auth: bool,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let url = format!(
            "{}/repos/{}/{}/issues",
            self.base_url, self.owner, self.repo
        );
        let mut request = self
            .http
            .get(url)
            .query(&[
                ("state", state.to_string()),
                ("per_page", per_page.to_string()),
                ("page", page.to_string()),
            ])
            .header(reqwest::header::ACCEPT, GITHUB_ACCEPT_HEADER)
            .header(reqwest::header::USER_AGENT, GITHUB_USER_AGENT);

        if include_auth {
            request = request.header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token),
            );
        }

        request.send().await
    }

    fn is_auth_failure(status: reqwest::StatusCode) -> bool {
        status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
    }
}

#[async_trait]
impl GitHubAdapter for GitHubApiAdapter {
    async fn list_issues(
        &self,
        query: &IssueListQuery,
    ) -> Result<Vec<RemoteIssue>, GitHubAdapterError> {
        let state = if query.state.trim().is_empty() {
            "open".to_string()
        } else {
            query.state.trim().to_string()
        };
        let (per_page, max_pages) = Self::normalize_pagination(query);

        let mut include_auth = !self.token.trim().is_empty();
        let mut retried_without_auth = false;
        let mut page = 1usize;
        let mut issues: Vec<RemoteIssue> = Vec::new();

        while page <= max_pages {
            let response = self
                .fetch_issue_page(&state, per_page, page, include_auth)
                .await?;
            let status = response.status();

            if include_auth && !retried_without_auth && Self::is_auth_failure(status) {
                include_auth = false;
                retried_without_auth = true;
                issues.clear();
                page = 1;
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(GitHubAdapterError::new(format!(
                    "github api returned {}: {}",
                    status, body
                )));
            }

            let items = response.json::<Vec<IssueListResponseItem>>().await?;
            if items.is_empty() {
                break;
            }
            let item_count = items.len();
            for item in items {
                if item.pull_request.is_some() {
                    continue;
                }

                let (milestone_number, milestone_title) = match item.milestone {
                    Some(m) => (Some(m.number), Some(m.title)),
                    None => (None, None),
                };
                issues.push(RemoteIssue {
                    number: item.number,
                    title: item.title,
                    state: item.state.to_uppercase(),
                    url: item.html_url,
                    labels: item.labels.into_iter().map(|label| label.name).collect(),
                    milestone_number,
                    milestone_title,
                    created_at: item.created_at,
                });
            }

            if item_count < per_page {
                break;
            }
            page += 1;
        }

        Ok(issues)
    }

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
            .header(reqwest::header::ACCEPT, GITHUB_ACCEPT_HEADER)
            .header(reqwest::header::USER_AGENT, GITHUB_USER_AGENT)
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
    remote_issues: Vec<RemoteIssue>,
}

impl InMemoryGitHubAdapter {
    pub fn recorded_payloads(&self) -> Vec<PrPayload> {
        self.inner
            .lock()
            .expect("in-memory github lock")
            .payloads
            .clone()
    }

    pub fn set_remote_issues(&self, issues: Vec<RemoteIssue>) {
        self.inner
            .lock()
            .expect("in-memory github lock")
            .remote_issues = issues;
    }
}

#[async_trait]
impl GitHubAdapter for InMemoryGitHubAdapter {
    async fn list_issues(
        &self,
        query: &IssueListQuery,
    ) -> Result<Vec<RemoteIssue>, GitHubAdapterError> {
        let state = query.state.trim().to_ascii_uppercase();
        let issues = self
            .inner
            .lock()
            .expect("in-memory github lock")
            .remote_issues
            .clone();
        let filtered = issues
            .into_iter()
            .filter(|issue| {
                if state.is_empty() || state == "ALL" {
                    true
                } else {
                    issue.state.eq_ignore_ascii_case(&state)
                }
            })
            .collect::<Vec<_>>();
        Ok(filtered)
    }

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
