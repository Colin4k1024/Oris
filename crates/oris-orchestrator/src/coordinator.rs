use std::fmt;
use std::sync::Arc;

use oris_agent_contract::A2aTaskLifecycleState;

use crate::evidence::{EvidenceBundle, ValidationGate};
use crate::github_adapter::{
    CreatedPullRequest, GitHubAdapter, GitHubAdapterError, GitHubApiAdapter, InMemoryGitHubAdapter,
    IssueListQuery, PrPayload, RemoteIssue,
};
use crate::issue_selection::select_next_issue;
use crate::runtime_client::{
    default_handshake_request, A2aSessionCompletion, A2aSessionRequest, HttpRuntimeA2aClient,
    InMemoryRuntimeA2aClient, RuntimeA2aClient, RuntimeClientError,
};
use crate::state::{transition, TaskState, TaskTransitionError};
use crate::task_spec::TaskSpec;

#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    pub sender_id: String,
    pub base_branch: String,
    pub branch_prefix: String,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            sender_id: "orchestrator".to_string(),
            base_branch: "main".to_string(),
            branch_prefix: "codex".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct Coordinator {
    runtime: Arc<dyn RuntimeA2aClient>,
    github: Arc<dyn GitHubAdapter>,
    config: CoordinatorConfig,
}

impl Coordinator {
    pub fn new(
        runtime: Arc<dyn RuntimeA2aClient>,
        github: Arc<dyn GitHubAdapter>,
        config: CoordinatorConfig,
    ) -> Self {
        Self {
            runtime,
            github,
            config,
        }
    }

    pub fn for_test() -> Self {
        Self::new(
            Arc::new(InMemoryRuntimeA2aClient::default()),
            Arc::new(InMemoryGitHubAdapter::default()),
            CoordinatorConfig::default(),
        )
    }

    pub fn with_http_clients(
        runtime_base_url: impl Into<String>,
        github_owner: impl Into<String>,
        github_repo: impl Into<String>,
        github_token: impl Into<String>,
        config: CoordinatorConfig,
    ) -> Self {
        Self::new(
            Arc::new(HttpRuntimeA2aClient::new(runtime_base_url)),
            Arc::new(GitHubApiAdapter::new(
                github_owner,
                github_repo,
                github_token,
            )),
            config,
        )
    }

    pub async fn select_next_remote_issue(&self) -> Result<SelectedIssue, CoordinatorError> {
        let issues = self
            .github
            .list_issues(&IssueListQuery::open_only())
            .await
            .map_err(CoordinatorError::github)?;
        let selected = select_next_issue(&issues)
            .ok_or_else(|| CoordinatorError::selection("no eligible remote issue found"))?;
        Ok(SelectedIssue::from_remote(selected))
    }

    pub async fn run_task(
        &self,
        spec: TaskSpec,
    ) -> Result<CoordinatorRunOutcome, CoordinatorError> {
        let handshake = self
            .runtime
            .handshake(default_handshake_request(&self.config.sender_id))
            .await
            .map_err(CoordinatorError::runtime)?;

        if !handshake.accepted {
            return Err(CoordinatorError::validation(
                "runtime rejected A2A handshake",
            ));
        }

        let start_request = A2aSessionRequest::start(
            &self.config.sender_id,
            crate::runtime_client::EXPECTED_PROTOCOL_VERSION,
            &spec.issue_id,
            &spec.title,
        );

        let start_ack = self
            .runtime
            .start_session(start_request)
            .await
            .map_err(CoordinatorError::runtime)?;

        let completion = A2aSessionCompletion::succeeded(
            &self.config.sender_id,
            "issue executed via orchestrator A2A flow",
            true,
        );

        let completion_response = self
            .runtime
            .complete_session(&start_ack.session_id, completion)
            .await
            .map_err(CoordinatorError::runtime)?;

        let is_success = matches!(
            completion_response.result.terminal_state,
            A2aTaskLifecycleState::Succeeded
        );
        let evidence =
            EvidenceBundle::new(&start_ack.session_id, is_success, is_success, is_success);

        if !ValidationGate::is_pr_ready(&evidence) {
            return Err(CoordinatorError::validation(
                "validation gate denied PR readiness",
            ));
        }

        let issue_slug = sanitize_issue_id(&spec.issue_id);
        let branch = format!(
            "{}/issue-{}",
            self.config.branch_prefix.trim_end_matches('/'),
            issue_slug
        );
        let body = format!(
            "Automated by orchestrator via A2A session {}.\nEvidence bundle: {}",
            start_ack.session_id,
            evidence.bundle_id()
        );
        let pr_payload = PrPayload::new(
            &spec.issue_id,
            &branch,
            &self.config.base_branch,
            &evidence.bundle_id(),
            &body,
        );

        pr_payload
            .validate()
            .map_err(CoordinatorError::validation)?;

        let pull_request = self
            .github
            .create_pull_request(&pr_payload)
            .await
            .map_err(CoordinatorError::github)?;

        let transition_state = transition(TaskState::Merged, "request_release")
            .map_err(CoordinatorError::state_transition)?;

        if transition_state != TaskState::ReleasePendingApproval {
            return Err(CoordinatorError::state_transition(
                TaskTransitionError::InvalidTransition,
            ));
        }

        Ok(CoordinatorRunOutcome {
            state: CoordinatorState::ReleasePendingApproval,
            session_id: start_ack.session_id,
            evidence,
            pull_request,
        })
    }

    pub async fn run_single_issue(
        &self,
        issue_id: &str,
    ) -> Result<CoordinatorState, CoordinatorError> {
        let spec = TaskSpec::new(
            issue_id,
            &format!("orchestrated task for {}", issue_id),
            vec![".".to_string()],
        )
        .map_err(CoordinatorError::task_spec)?;

        let outcome = self.run_task(spec).await?;
        Ok(outcome.state)
    }

    pub async fn run_next_remote_issue(&self) -> Result<SelectedIssueRunOutcome, CoordinatorError> {
        let selected_issue = self.select_next_remote_issue().await?;
        let spec = TaskSpec::new(
            &selected_issue.issue_id(),
            &selected_issue.title,
            vec![".".to_string()],
        )
        .map_err(CoordinatorError::task_spec)?;
        let run_outcome = self.run_task(spec).await?;
        Ok(SelectedIssueRunOutcome {
            selected_issue,
            run_outcome,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorRunOutcome {
    pub state: CoordinatorState,
    pub session_id: String,
    pub evidence: EvidenceBundle,
    pub pull_request: CreatedPullRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedIssue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub labels: Vec<String>,
    pub milestone_number: Option<u64>,
    pub milestone_title: Option<String>,
    pub created_at: Option<String>,
}

impl SelectedIssue {
    pub fn issue_id(&self) -> String {
        format!("issue-{}", self.number)
    }

    fn from_remote(issue: RemoteIssue) -> Self {
        Self {
            number: issue.number,
            title: issue.title,
            state: issue.state,
            url: issue.url,
            labels: issue.labels,
            milestone_number: issue.milestone_number,
            milestone_title: issue.milestone_title,
            created_at: issue.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedIssueRunOutcome {
    pub selected_issue: SelectedIssue,
    pub run_outcome: CoordinatorRunOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorState {
    ReleasePendingApproval,
}

impl CoordinatorState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReleasePendingApproval => "ReleasePendingApproval",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorError {
    kind: &'static str,
    message: String,
}

impl CoordinatorError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn task_spec(message: impl Into<String>) -> Self {
        Self::new("task_spec", message)
    }

    fn runtime(error: RuntimeClientError) -> Self {
        Self::new("runtime", error.to_string())
    }

    fn github(error: GitHubAdapterError) -> Self {
        Self::new("github", error.to_string())
    }

    fn validation(message: impl Into<String>) -> Self {
        Self::new("validation", message)
    }

    fn selection(message: impl Into<String>) -> Self {
        Self::new("selection", message)
    }

    fn state_transition(error: TaskTransitionError) -> Self {
        Self::new("state_transition", format!("{:?}", error))
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }
}

impl fmt::Display for CoordinatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for CoordinatorError {}

fn sanitize_issue_id(issue_id: &str) -> String {
    let mut out = String::with_capacity(issue_id.len());
    for ch in issue_id.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}
