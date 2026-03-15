//! Intake source definitions and implementations

use crate::{IntakeError, IntakeResult, IntakeSourceConfig};
use serde::{Deserialize, Serialize};

/// Supported intake source types
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IntakeSourceType {
    /// GitHub Actions webhooks
    Github,
    /// GitLab CI webhooks
    Gitlab,
    /// Prometheus/AlertManager alerts
    Prometheus,
    /// Sentry error tracking
    Sentry,
    /// Generic HTTP webhook
    Http,
    /// Log file monitoring
    LogFile,
}

impl Default for IntakeSourceType {
    fn default() -> Self {
        Self::Http
    }
}

impl std::fmt::Display for IntakeSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntakeSourceType::Github => write!(f, "github"),
            IntakeSourceType::Gitlab => write!(f, "gitlab"),
            IntakeSourceType::Prometheus => write!(f, "prometheus"),
            IntakeSourceType::Sentry => write!(f, "sentry"),
            IntakeSourceType::Http => write!(f, "http"),
            IntakeSourceType::LogFile => write!(f, "logfile"),
        }
    }
}

/// Trait for implementing intake sources
pub trait IntakeSource: Send + Sync {
    /// Get the source type
    fn source_type(&self) -> IntakeSourceType;

    /// Process incoming data and extract potential issues
    fn process(&self, payload: &[u8]) -> IntakeResult<Vec<IntakeEvent>>;

    /// Validate the incoming data format
    fn validate(&self, payload: &[u8]) -> IntakeResult<()>;
}

/// An intake event represents a detected issue from an external source
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntakeEvent {
    /// Unique identifier for this event
    pub event_id: String,

    /// Source type that generated this event
    pub source_type: IntakeSourceType,

    /// Original source event ID (if available)
    pub source_event_id: Option<String>,

    /// Title/description of the detected issue
    pub title: String,

    /// Detailed description
    pub description: String,

    /// Severity level
    pub severity: IssueSeverity,

    /// Extracted signals from this event
    pub signals: Vec<String>,

    /// Raw payload (for debugging)
    pub raw_payload: Option<String>,

    /// Timestamp when the event was generated
    pub timestamp_ms: i64,
}

/// Issue severity levels
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Default for IssueSeverity {
    fn default() -> Self {
        Self::Medium
    }
}

impl std::fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueSeverity::Critical => write!(f, "critical"),
            IssueSeverity::High => write!(f, "high"),
            IssueSeverity::Medium => write!(f, "medium"),
            IssueSeverity::Low => write!(f, "low"),
            IssueSeverity::Info => write!(f, "info"),
        }
    }
}

/// GitHub Actions webhook payload
#[derive(Clone, Debug, Deserialize)]
pub struct GithubWorkflowEvent {
    pub action: Option<String>,
    pub workflow: Option<String>,
    pub run_id: Option<i64>,
    pub repository: Option<GithubRepository>,
    pub workflow_run: Option<GithubWorkflowRun>,
    pub conclusion: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GithubRepository {
    pub full_name: String,
    pub html_url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GithubWorkflowRun {
    pub head_branch: String,
    pub head_sha: String,
    pub html_url: String,
    pub logs_url: String,
    pub artifacts_url: String,
}

/// GitLab CI webhook payload
#[derive(Clone, Debug, Deserialize)]
pub struct GitlabPipelineEvent {
    pub object_kind: Option<String>,
    pub object_attributes: Option<GitlabPipelineAttributes>,
    pub project: Option<GitlabProject>,
    pub builds: Option<Vec<GitlabBuild>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitlabPipelineAttributes {
    pub id: i64,
    pub ref_: String,
    pub sha: String,
    pub status: String,
    pub finished_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitlabProject {
    pub id: i64,
    pub name: String,
    pub path_with_namespace: String,
    pub web_url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitlabBuild {
    pub id: i64,
    pub name: String,
    pub stage: String,
    pub status: String,
}

/// Generic HTTP webhook event
#[derive(Clone, Debug, Deserialize)]
pub struct HttpWebhookEvent {
    /// Event type
    pub event_type: String,
    /// Event ID
    pub event_id: String,
    /// Event timestamp
    pub timestamp: Option<i64>,
    /// Payload
    pub payload: serde_json::Value,
}

/// Build an intake event from GitHub Actions webhook
pub fn from_github_workflow(event: GithubWorkflowEvent) -> IntakeResult<IntakeEvent> {
    let workflow_name = event.workflow.clone().unwrap_or_default();
    let title = format!(
        "GitHub Workflow {} - {}",
        event.action.unwrap_or_default(),
        workflow_name
    );

    let conclusion = event.conclusion.as_deref().unwrap_or("unknown");
    let severity = match conclusion {
        "failure" => IssueSeverity::High,
        "cancelled" => IssueSeverity::Medium,
        "success" => IssueSeverity::Low,
        _ => IssueSeverity::Info,
    };

    let mut signals = vec![];

    // Extract signals based on workflow conclusion
    if let Some(conc) = event.conclusion.as_ref() {
        signals.push(format!("workflow_conclusion:{}", conc));
    }

    if let Some(run_id) = event.run_id {
        signals.push(format!("run_id:{}", run_id));
    }

    let description = format!(
        "Workflow '{}' concluded with '{}' for repository '{}'",
        workflow_name,
        conclusion,
        event
            .repository
            .as_ref()
            .map(|r| r.full_name.clone())
            .unwrap_or_default()
    );

    Ok(IntakeEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        source_type: IntakeSourceType::Github,
        source_event_id: event.run_id.map(|id| id.to_string()),
        title,
        description,
        severity,
        signals,
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    })
}

/// Build an intake event from GitLab CI webhook
pub fn from_gitlab_pipeline(event: GitlabPipelineEvent) -> IntakeResult<IntakeEvent> {
    let pipeline = event.object_attributes.as_ref();
    let project = event.project.as_ref();

    let title = format!(
        "GitLab Pipeline {} - {}",
        pipeline.map(|p| p.status.clone()).unwrap_or_default(),
        pipeline.map(|p| p.ref_.clone()).unwrap_or_default()
    );

    let status = pipeline.map(|p| p.status.as_str()).unwrap_or("unknown");
    let severity = match status {
        "failed" => IssueSeverity::High,
        "canceled" => IssueSeverity::Medium,
        "success" => IssueSeverity::Low,
        _ => IssueSeverity::Info,
    };

    let mut signals = vec![];
    signals.push(format!("pipeline_status:{}", status));

    if let Some(p) = pipeline {
        signals.push(format!("pipeline_id:{}", p.id));
        signals.push(format!("commit_sha:{}", p.sha));
    }

    let description = format!(
        "Pipeline '{}' on branch '{}' for project '{}'",
        pipeline.map(|p| p.id.to_string()).unwrap_or_default(),
        pipeline.map(|p| p.ref_.clone()).unwrap_or_default(),
        project
            .map(|p| p.path_with_namespace.clone())
            .unwrap_or_default()
    );

    Ok(IntakeEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        source_type: IntakeSourceType::Gitlab,
        source_event_id: pipeline.map(|p| p.id.to_string()),
        title,
        description,
        severity,
        signals,
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    })
}

/// GitHub check_run event payload (check_run failed / completed)
#[derive(Clone, Debug, Deserialize)]
pub struct GithubCheckRunEvent {
    pub action: Option<String>,
    pub check_run: Option<GithubCheckRun>,
    pub repository: Option<GithubRepository>,
}

/// Details of a single check run
#[derive(Clone, Debug, Deserialize)]
pub struct GithubCheckRun {
    pub id: i64,
    pub name: String,
    pub head_sha: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: Option<String>,
    pub output: Option<GithubCheckRunOutput>,
}

/// Check run log output summary
#[derive(Clone, Debug, Deserialize)]
pub struct GithubCheckRunOutput {
    pub title: Option<String>,
    pub summary: Option<String>,
}

/// Convert a GitHub check_run event into an intake event.
pub fn from_github_check_run(event: GithubCheckRunEvent) -> IntakeResult<IntakeEvent> {
    let check = event.check_run.as_ref();
    let check_name = check.map(|c| c.name.as_str()).unwrap_or("unknown");
    let conclusion = check
        .and_then(|c| c.conclusion.as_deref())
        .unwrap_or("unknown");

    let severity = match conclusion {
        "failure" | "timed_out" => IssueSeverity::High,
        "cancelled" | "action_required" => IssueSeverity::Medium,
        "success" | "neutral" | "skipped" => IssueSeverity::Low,
        _ => IssueSeverity::Info,
    };

    let output_title = check
        .and_then(|c| c.output.as_ref())
        .and_then(|o| o.title.as_deref())
        .unwrap_or("");
    let output_summary = check
        .and_then(|c| c.output.as_ref())
        .and_then(|o| o.summary.as_deref())
        .unwrap_or("");

    let title = format!("GitHub check_run '{}' {}", check_name, conclusion);
    let description = format!(
        "Check '{}' concluded '{}' on commit '{}' for repository '{}'. {}: {}",
        check_name,
        conclusion,
        check.map(|c| c.head_sha.as_str()).unwrap_or(""),
        event
            .repository
            .as_ref()
            .map(|r| r.full_name.as_str())
            .unwrap_or(""),
        output_title,
        output_summary
    );

    let mut signals = vec![format!("check_run_conclusion:{}", conclusion)];
    if let Some(c) = check {
        signals.push(format!("check_run_name:{}", c.name));
        signals.push(format!("commit_sha:{}", c.head_sha));
    }
    if !output_title.is_empty() {
        signals.push(format!("output_title:{}", output_title));
    }
    if !output_summary.is_empty() {
        signals.push(format!("output_summary:{}", output_summary));
    }

    Ok(IntakeEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        source_type: IntakeSourceType::Github,
        source_event_id: check.map(|c| c.id.to_string()),
        title,
        description,
        severity,
        signals,
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    })
}

/// GitHub intake source — handles `workflow_run` and `check_run` webhook payloads.
///
/// Dispatches on the `X-GitHub-Event` header. Because this source operates over
/// raw bytes, it accepts an optional `event_type` hint at construction time that
/// mirrors the `X-GitHub-Event` header value (`"workflow_run"` or `"check_run"`).
#[derive(Clone, Debug)]
pub struct GithubIntakeSource {
    /// The expected GitHub event type (mirrors X-GitHub-Event header).
    /// Defaults to dispatching by presence of known top-level keys when `None`.
    pub event_type: Option<String>,
}

impl GithubIntakeSource {
    /// Create a new source that processes the given GitHub event type.
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: Some(event_type.into()),
        }
    }

    /// Create a source that auto-detects the event type from payload shape.
    pub fn auto() -> Self {
        Self { event_type: None }
    }

    fn dispatch(&self, payload: &[u8]) -> IntakeResult<Vec<IntakeEvent>> {
        let value: serde_json::Value =
            serde_json::from_slice(payload).map_err(|e| IntakeError::ParseError(e.to_string()))?;

        let hint = self.event_type.as_deref().unwrap_or_else(|| {
            // Auto-detect: check_run events have a `check_run` top-level key
            if value.get("check_run").is_some() {
                "check_run"
            } else {
                "workflow_run"
            }
        });

        match hint {
            "check_run" => {
                let ev: GithubCheckRunEvent = serde_json::from_value(value)
                    .map_err(|e| IntakeError::ParseError(e.to_string()))?;
                from_github_check_run(ev).map(|e| vec![e])
            }
            _ => {
                let ev: GithubWorkflowEvent = serde_json::from_value(value)
                    .map_err(|e| IntakeError::ParseError(e.to_string()))?;
                from_github_workflow(ev).map(|e| vec![e])
            }
        }
    }
}

impl IntakeSource for GithubIntakeSource {
    fn source_type(&self) -> IntakeSourceType {
        IntakeSourceType::Github
    }

    fn process(&self, payload: &[u8]) -> IntakeResult<Vec<IntakeEvent>> {
        self.dispatch(payload)
    }

    fn validate(&self, payload: &[u8]) -> IntakeResult<()> {
        // Require valid UTF-8 JSON
        let _: serde_json::Value = serde_json::from_slice(payload)
            .map_err(|e| IntakeError::ParseError(format!("invalid JSON: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_severity_display() {
        assert_eq!(IssueSeverity::Critical.to_string(), "critical");
        assert_eq!(IssueSeverity::High.to_string(), "high");
    }

    #[test]
    fn test_github_event_conversion() {
        let event = GithubWorkflowEvent {
            action: Some("completed".to_string()),
            workflow: Some("ci.yml".to_string()),
            run_id: Some(12345),
            repository: Some(GithubRepository {
                full_name: "owner/repo".to_string(),
                html_url: "https://github.com/owner/repo".to_string(),
            }),
            workflow_run: Some(GithubWorkflowRun {
                head_branch: "main".to_string(),
                head_sha: "abc123".to_string(),
                html_url: "https://github.com/owner/repo/actions/runs/12345".to_string(),
                logs_url: "https://api.github.com/owner/repo/actions/runs/12345/logs".to_string(),
                artifacts_url: "https://api.github.com/owner/repo/actions/runs/12345/artifacts"
                    .to_string(),
            }),
            conclusion: Some("failure".to_string()),
        };

        let intake = from_github_workflow(event).unwrap();
        assert_eq!(intake.severity, IssueSeverity::High);
        assert!(intake.signals.iter().any(|s| s.contains("failure")));
    }
}
