//! Intake source definitions and implementations

use crate::{IntakeError, IntakeResult, IntakeSourceConfig};
use regex_lite::Regex;
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

/// Log-file intake source for structured application or CI logs.
///
/// The current implementation scans UTF-8 log lines and emits one `IntakeEvent`
/// for each line matching common failure patterns such as `error`, `panic`,
/// `fatal`, or `exception`.
#[derive(Clone, Debug)]
pub struct LogFileIntakeSource {
    patterns: Vec<Regex>,
}

impl LogFileIntakeSource {
    /// Create a log intake source with default error-oriented patterns.
    pub fn new() -> Self {
        Self {
            patterns: vec![
                Regex::new(r"(?i)\berror\b").unwrap(),
                Regex::new(r"(?i)\bpanic\b").unwrap(),
                Regex::new(r"(?i)\bfatal\b").unwrap(),
                Regex::new(r"(?i)\bexception\b").unwrap(),
                Regex::new(r"(?i)test\s+failed").unwrap(),
            ],
        }
    }

    fn severity_for_line(&self, line: &str) -> IssueSeverity {
        let lower = line.to_ascii_lowercase();
        if lower.contains("panic") || lower.contains("fatal") {
            IssueSeverity::High
        } else if lower.contains("error") || lower.contains("exception") {
            IssueSeverity::Medium
        } else {
            IssueSeverity::Low
        }
    }

    fn matches(&self, line: &str) -> bool {
        self.patterns.iter().any(|pattern| pattern.is_match(line))
    }
}

impl Default for LogFileIntakeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl IntakeSource for LogFileIntakeSource {
    fn source_type(&self) -> IntakeSourceType {
        IntakeSourceType::LogFile
    }

    fn process(&self, payload: &[u8]) -> IntakeResult<Vec<IntakeEvent>> {
        let contents = std::str::from_utf8(payload)
            .map_err(|e| IntakeError::ParseError(format!("invalid UTF-8 log payload: {}", e)))?;

        let events = contents
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let trimmed = line.trim();
                if trimmed.is_empty() || !self.matches(trimmed) {
                    return None;
                }

                Some(IntakeEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    source_type: IntakeSourceType::LogFile,
                    source_event_id: Some(format!("line:{}", index + 1)),
                    title: format!("LogFile error at line {}", index + 1),
                    description: trimmed.to_string(),
                    severity: self.severity_for_line(trimmed),
                    signals: vec![format!("log_match:{}", trimmed)],
                    raw_payload: Some(trimmed.to_string()),
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                })
            })
            .collect();

        Ok(events)
    }

    fn validate(&self, payload: &[u8]) -> IntakeResult<()> {
        std::str::from_utf8(payload)
            .map_err(|e| IntakeError::ParseError(format!("invalid UTF-8 log payload: {}", e)))?;
        Ok(())
    }
}

/// Prometheus/Alertmanager v4 webhook payload.
///
/// See https://prometheus.io/docs/alerting/latest/configuration/#webhook_config
#[derive(Clone, Debug, Deserialize)]
pub struct AlertmanagerPayload {
    pub version: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "groupLabels")]
    pub group_labels: Option<std::collections::HashMap<String, String>>,
    #[serde(rename = "commonLabels")]
    pub common_labels: Option<std::collections::HashMap<String, String>>,
    #[serde(rename = "commonAnnotations")]
    pub common_annotations: Option<std::collections::HashMap<String, String>>,
    pub alerts: Option<Vec<AlertmanagerAlert>>,
    #[serde(rename = "externalURL")]
    pub external_url: Option<String>,
}

/// Single alert within an Alertmanager payload.
#[derive(Clone, Debug, Deserialize)]
pub struct AlertmanagerAlert {
    pub status: Option<String>,
    pub labels: Option<std::collections::HashMap<String, String>>,
    pub annotations: Option<std::collections::HashMap<String, String>>,
    pub fingerprint: Option<String>,
}

/// Prometheus/Alertmanager intake source.
///
/// Processes Alertmanager webhook v4 payloads and converts firing alerts to
/// `IntakeEvent` instances. One event is emitted per alert in the payload.
#[derive(Clone, Debug, Default)]
pub struct PrometheusIntakeSource;

impl PrometheusIntakeSource {
    fn alert_severity(labels: &std::collections::HashMap<String, String>) -> IssueSeverity {
        match labels.get("severity").map(|s| s.as_str()).unwrap_or("") {
            "critical" | "page" => IssueSeverity::Critical,
            "warning" | "high" => IssueSeverity::High,
            "info" | "low" => IssueSeverity::Low,
            _ => IssueSeverity::Medium,
        }
    }
}

impl IntakeSource for PrometheusIntakeSource {
    fn source_type(&self) -> IntakeSourceType {
        IntakeSourceType::Prometheus
    }

    fn process(&self, payload: &[u8]) -> IntakeResult<Vec<IntakeEvent>> {
        let alert_payload: AlertmanagerPayload = serde_json::from_slice(payload)
            .map_err(|e| IntakeError::ParseError(format!("invalid Alertmanager JSON: {}", e)))?;

        let group_status = alert_payload.status.as_deref().unwrap_or("unknown");
        let alerts = alert_payload.alerts.unwrap_or_default();
        let empty_map = std::collections::HashMap::new();

        let events = alerts
            .into_iter()
            .filter(|a| a.status.as_deref().unwrap_or("") == "firing")
            .map(|alert| {
                let labels = alert.labels.as_ref().unwrap_or(&empty_map);
                let annotations = alert.annotations.as_ref().unwrap_or(&empty_map);
                let alert_name = labels
                    .get("alertname")
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");
                let summary = annotations
                    .get("summary")
                    .or_else(|| annotations.get("message"))
                    .map(|s| s.as_str())
                    .unwrap_or("Prometheus alert fired");
                let description = annotations
                    .get("description")
                    .or_else(|| annotations.get("runbook_url"))
                    .map(|s| s.as_str())
                    .unwrap_or(summary);

                let severity = Self::alert_severity(labels);

                let signals: Vec<String> = labels
                    .iter()
                    .map(|(k, v)| format!("label:{}:{}", k, v))
                    .chain(
                        alert
                            .fingerprint
                            .as_ref()
                            .map(|fp| format!("fingerprint:{}", fp))
                            .into_iter(),
                    )
                    .collect();

                IntakeEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    source_type: IntakeSourceType::Prometheus,
                    source_event_id: alert.fingerprint.clone(),
                    title: format!("Alert: {} [{}]", alert_name, group_status),
                    description: description.to_string(),
                    severity,
                    signals,
                    raw_payload: None,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                }
            })
            .collect();

        Ok(events)
    }

    fn validate(&self, payload: &[u8]) -> IntakeResult<()> {
        let _: serde_json::Value = serde_json::from_slice(payload)
            .map_err(|e| IntakeError::ParseError(format!("invalid JSON: {}", e)))?;
        Ok(())
    }
}

/// Minimal Sentry issue alert webhook payload.
#[derive(Clone, Debug, Deserialize)]
pub struct SentryAlertPayload {
    pub action: Option<String>,
    pub actor: Option<SentryActor>,
    pub data: Option<SentryAlertData>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SentryActor {
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SentryAlertData {
    pub issue: Option<SentryIssue>,
    pub error: Option<SentryError>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SentryIssue {
    pub id: Option<String>,
    pub title: Option<String>,
    pub level: Option<String>,
    pub project: Option<SentryProject>,
    pub permalink: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SentryProject {
    pub slug: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SentryError {
    pub message: Option<String>,
    pub level: Option<String>,
}

/// Sentry issue alert intake source.
///
/// Handles Sentry issue-alert webhook payloads (action: created/resolved).
/// Emits one `IntakeEvent` per webhook delivery.
#[derive(Clone, Debug, Default)]
pub struct SentryIntakeSource;

impl SentryIntakeSource {
    fn level_to_severity(level: &str) -> IssueSeverity {
        match level {
            "fatal" | "critical" => IssueSeverity::Critical,
            "error" => IssueSeverity::High,
            "warning" => IssueSeverity::Medium,
            "info" | "debug" => IssueSeverity::Low,
            _ => IssueSeverity::Medium,
        }
    }
}

impl IntakeSource for SentryIntakeSource {
    fn source_type(&self) -> IntakeSourceType {
        IntakeSourceType::Sentry
    }

    fn process(&self, payload: &[u8]) -> IntakeResult<Vec<IntakeEvent>> {
        let alert: SentryAlertPayload = serde_json::from_slice(payload)
            .map_err(|e| IntakeError::ParseError(format!("invalid Sentry JSON: {}", e)))?;

        let action = alert.action.as_deref().unwrap_or("unknown");
        // Only process "created" (new error) and "triggered" (alert rule) actions.
        if !matches!(action, "created" | "triggered") {
            return Ok(vec![]);
        }

        let data = alert.data.as_ref();
        let issue = data.and_then(|d| d.issue.as_ref());
        let error_data = data.and_then(|d| d.error.as_ref());

        let (title, level, event_id, description) = if let Some(issue) = issue {
            let lvl = issue.level.as_deref().unwrap_or("error");
            let project = issue
                .project
                .as_ref()
                .and_then(|p| p.slug.as_deref())
                .unwrap_or("unknown");
            let title = issue.title.as_deref().unwrap_or("Sentry issue").to_string();
            let id = issue.id.clone();
            let desc = issue
                .permalink
                .as_deref()
                .map(|url| format!("View issue: {}", url))
                .unwrap_or_else(|| format!("Sentry [{}] {} in project {}", lvl, title, project));
            (title, lvl.to_string(), id, desc)
        } else if let Some(err) = error_data {
            let lvl = err.level.as_deref().unwrap_or("error");
            let msg = err.message.as_deref().unwrap_or("Sentry error");
            (msg.to_string(), lvl.to_string(), None, msg.to_string())
        } else {
            return Ok(vec![]);
        };

        let severity = Self::level_to_severity(&level);
        let signals = vec![
            format!("sentry_action:{}", action),
            format!("sentry_level:{}", level),
        ];

        Ok(vec![IntakeEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            source_type: IntakeSourceType::Sentry,
            source_event_id: event_id,
            title: format!("Sentry: {}", title),
            description,
            severity,
            signals,
            raw_payload: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }])
    }

    fn validate(&self, payload: &[u8]) -> IntakeResult<()> {
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

    #[test]
    fn test_log_file_intake_source_extracts_matching_lines() {
        let source = LogFileIntakeSource::new();
        let payload = b"info startup complete\nERROR database unavailable\npanic: worker crashed\n";

        let events = source.process(payload).expect("log file should parse");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].source_type, IntakeSourceType::LogFile);
        assert!(events[0].description.contains("ERROR database unavailable"));
        assert_eq!(events[1].severity, IssueSeverity::High);
    }

    #[test]
    fn test_prometheus_and_sentry_stubs_validate_json() {
        let prometheus = PrometheusIntakeSource;
        let sentry = SentryIntakeSource;

        assert!(prometheus.validate(br#"{"alerts":[]}"#).is_ok());
        assert!(sentry.validate(br#"{"event_id":"abc"}"#).is_ok());
    }

    #[test]
    fn test_prometheus_intake_source_parses_firing_alert() {
        let source = PrometheusIntakeSource;
        let payload = br#"{
            "version": "4",
            "status": "firing",
            "commonLabels": {"severity": "critical"},
            "commonAnnotations": {"summary": "DB down"},
            "alerts": [
                {
                    "status": "firing",
                    "labels": {"alertname": "DBDown", "severity": "critical"},
                    "annotations": {"summary": "Database is unreachable"},
                    "fingerprint": "abc123"
                },
                {
                    "status": "resolved",
                    "labels": {"alertname": "DBDown", "severity": "critical"},
                    "annotations": {"summary": "Database is unreachable"},
                    "fingerprint": "abc124"
                }
            ]
        }"#;
        let events = source
            .process(payload)
            .expect("should parse alertmanager payload");
        // Only firing alerts should produce events
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].severity, IssueSeverity::Critical);
        assert!(events[0].title.contains("DBDown"));
        assert_eq!(events[0].source_type, IntakeSourceType::Prometheus);
        assert_eq!(events[0].source_event_id, Some("abc123".to_string()));
    }

    #[test]
    fn test_prometheus_intake_source_empty_alerts() {
        let source = PrometheusIntakeSource;
        let payload = br#"{"version":"4","status":"resolved","alerts":[]}"#;
        let events = source.process(payload).expect("empty alerts ok");
        assert!(events.is_empty());
    }

    #[test]
    fn test_sentry_intake_source_parses_issue_created() {
        let source = SentryIntakeSource;
        let payload = br#"{
            "action": "created",
            "data": {
                "issue": {
                    "id": "sentry-issue-1",
                    "title": "ZeroDivisionError",
                    "level": "error",
                    "project": {"slug": "my-app", "name": "My App"},
                    "permalink": "https://sentry.io/org/my-app/issues/1"
                }
            }
        }"#;
        let events = source.process(payload).expect("sentry parse ok");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].severity, IssueSeverity::High);
        assert!(events[0].title.contains("ZeroDivisionError"));
        assert_eq!(events[0].source_type, IntakeSourceType::Sentry);
    }

    #[test]
    fn test_sentry_intake_source_resolved_action_skipped() {
        let source = SentryIntakeSource;
        let payload = br#"{"action": "resolved", "data": {"issue": {"id": "1", "title": "Err", "level": "error"}}}"#;
        let events = source.process(payload).expect("resolved parse ok");
        assert!(events.is_empty());
    }
}
