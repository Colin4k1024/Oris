use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use oris_agent_contract::{
    A2aCapability, A2aHandshakeRequest, A2aHandshakeResponse, A2aProtocol, A2aTaskLifecycleState,
    A2aTaskSessionAck, A2aTaskSessionCompletionRequest, A2aTaskSessionCompletionResponse,
    A2aTaskSessionResult, A2aTaskSessionStartRequest, AgentCapabilityLevel, AgentRole,
    ReplayFeedback, ReplayPlannerDirective, A2A_TASK_SESSION_PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;

pub const EXPECTED_PROTOCOL_VERSION: &str = A2A_TASK_SESSION_PROTOCOL_VERSION;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A2aSessionRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub task_id: String,
    pub task_summary: String,
}

impl A2aSessionRequest {
    pub fn start(
        sender_id: &str,
        protocol_version: &str,
        task_id: &str,
        task_summary: &str,
    ) -> Self {
        Self {
            sender_id: sender_id.to_string(),
            protocol_version: protocol_version.to_string(),
            task_id: task_id.to_string(),
            task_summary: task_summary.to_string(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol_version != EXPECTED_PROTOCOL_VERSION {
            return Err("incompatible a2a task session protocol version");
        }
        Ok(())
    }

    pub fn into_contract(self) -> A2aTaskSessionStartRequest {
        A2aTaskSessionStartRequest {
            sender_id: self.sender_id,
            protocol_version: self.protocol_version,
            task_id: self.task_id,
            task_summary: self.task_summary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A2aSessionCompletion {
    pub sender_id: String,
    pub protocol_version: String,
    pub terminal_state: A2aTaskLifecycleState,
    pub summary: String,
    pub used_capsule: bool,
    pub capsule_id: Option<String>,
    pub fallback_reason: Option<String>,
    pub task_class_id: String,
    pub task_label: String,
}

impl A2aSessionCompletion {
    pub fn succeeded(sender_id: &str, summary: &str, used_capsule: bool) -> Self {
        Self {
            sender_id: sender_id.to_string(),
            protocol_version: EXPECTED_PROTOCOL_VERSION.to_string(),
            terminal_state: A2aTaskLifecycleState::Succeeded,
            summary: summary.to_string(),
            used_capsule,
            capsule_id: None,
            fallback_reason: None,
            task_class_id: "issue-automation".to_string(),
            task_label: "issue-automation".to_string(),
        }
    }

    pub fn into_contract(self) -> A2aTaskSessionCompletionRequest {
        A2aTaskSessionCompletionRequest {
            sender_id: self.sender_id,
            protocol_version: self.protocol_version,
            terminal_state: self.terminal_state,
            summary: self.summary,
            retryable: false,
            retry_after_ms: None,
            failure_code: None,
            failure_details: None,
            used_capsule: self.used_capsule,
            capsule_id: self.capsule_id,
            reasoning_steps_avoided: if self.used_capsule { 1 } else { 0 },
            fallback_reason: self.fallback_reason,
            task_class_id: self.task_class_id,
            task_label: self.task_label,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeClientError {
    message: String,
}

impl RuntimeClientError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RuntimeClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeClientError {}

impl From<reqwest::Error> for RuntimeClientError {
    fn from(value: reqwest::Error) -> Self {
        Self::new(format!("runtime http error: {}", value))
    }
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    data: T,
}

#[async_trait]
pub trait RuntimeA2aClient: Send + Sync {
    async fn handshake(
        &self,
        request: A2aHandshakeRequest,
    ) -> Result<A2aHandshakeResponse, RuntimeClientError>;

    async fn start_session(
        &self,
        request: A2aSessionRequest,
    ) -> Result<A2aTaskSessionAck, RuntimeClientError>;

    async fn complete_session(
        &self,
        session_id: &str,
        request: A2aSessionCompletion,
    ) -> Result<A2aTaskSessionCompletionResponse, RuntimeClientError>;
}

#[derive(Clone)]
pub struct HttpRuntimeA2aClient {
    base_url: String,
    http: reqwest::Client,
}

impl HttpRuntimeA2aClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    async fn post<TReq, TResp>(
        &self,
        path: &str,
        payload: &TReq,
    ) -> Result<TResp, RuntimeClientError>
    where
        TReq: serde::Serialize + ?Sized,
        TResp: DeserializeOwned,
    {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let response = self.http.post(url).json(payload).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeClientError::new(format!(
                "runtime api returned {}: {}",
                status, body
            )));
        }
        let envelope = response.json::<ApiEnvelope<TResp>>().await?;
        Ok(envelope.data)
    }
}

#[async_trait]
impl RuntimeA2aClient for HttpRuntimeA2aClient {
    async fn handshake(
        &self,
        request: A2aHandshakeRequest,
    ) -> Result<A2aHandshakeResponse, RuntimeClientError> {
        self.post("/v1/evolution/a2a/handshake", &request).await
    }

    async fn start_session(
        &self,
        request: A2aSessionRequest,
    ) -> Result<A2aTaskSessionAck, RuntimeClientError> {
        request
            .validate()
            .map_err(|e| RuntimeClientError::new(e.to_string()))?;
        self.post("/v1/evolution/a2a/sessions/start", &request.into_contract())
            .await
    }

    async fn complete_session(
        &self,
        session_id: &str,
        request: A2aSessionCompletion,
    ) -> Result<A2aTaskSessionCompletionResponse, RuntimeClientError> {
        if request.protocol_version != EXPECTED_PROTOCOL_VERSION {
            return Err(RuntimeClientError::new(
                "incompatible a2a task session protocol version",
            ));
        }
        self.post(
            &format!("/v1/evolution/a2a/sessions/{}/complete", session_id),
            &request.into_contract(),
        )
        .await
    }
}

#[derive(Clone, Default)]
pub struct InMemoryRuntimeA2aClient {
    inner: Arc<Mutex<InMemoryRuntimeState>>,
}

#[derive(Default)]
struct InMemoryRuntimeState {
    next_session_id: u64,
    sessions: HashMap<String, A2aTaskSessionAck>,
    completions: HashMap<String, A2aTaskSessionCompletionResponse>,
    accepted_handshakes: usize,
}

impl InMemoryRuntimeA2aClient {
    pub fn accepted_handshakes(&self) -> usize {
        self.inner
            .lock()
            .expect("in-memory runtime lock")
            .accepted_handshakes
    }

    pub fn completion(&self, session_id: &str) -> Option<A2aTaskSessionCompletionResponse> {
        self.inner
            .lock()
            .expect("in-memory runtime lock")
            .completions
            .get(session_id)
            .cloned()
    }
}

#[async_trait]
impl RuntimeA2aClient for InMemoryRuntimeA2aClient {
    async fn handshake(
        &self,
        request: A2aHandshakeRequest,
    ) -> Result<A2aHandshakeResponse, RuntimeClientError> {
        if !request.supports_current_protocol() {
            return Err(RuntimeClientError::new(
                "incompatible a2a handshake protocol",
            ));
        }

        let mut state = self.inner.lock().expect("in-memory runtime lock");
        state.accepted_handshakes += 1;

        Ok(A2aHandshakeResponse::accept(vec![
            A2aCapability::Coordination,
            A2aCapability::SupervisedDevloop,
            A2aCapability::ReplayFeedback,
        ]))
    }

    async fn start_session(
        &self,
        request: A2aSessionRequest,
    ) -> Result<A2aTaskSessionAck, RuntimeClientError> {
        request
            .validate()
            .map_err(|e| RuntimeClientError::new(e.to_string()))?;

        let mut state = self.inner.lock().expect("in-memory runtime lock");
        state.next_session_id += 1;

        let session_id = format!("a2a-session-{}", state.next_session_id);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_millis() as u64;

        let ack = A2aTaskSessionAck {
            session_id: session_id.clone(),
            task_id: request.task_id,
            state: oris_agent_contract::A2aTaskSessionState::Started,
            summary: request.task_summary,
            retryable: false,
            retry_after_ms: None,
            updated_at_ms: now,
        };

        state.sessions.insert(session_id, ack.clone());
        Ok(ack)
    }

    async fn complete_session(
        &self,
        session_id: &str,
        request: A2aSessionCompletion,
    ) -> Result<A2aTaskSessionCompletionResponse, RuntimeClientError> {
        if request.protocol_version != EXPECTED_PROTOCOL_VERSION {
            return Err(RuntimeClientError::new(
                "incompatible a2a task session protocol version",
            ));
        }

        let mut state = self.inner.lock().expect("in-memory runtime lock");
        let session =
            state.sessions.get(session_id).cloned().ok_or_else(|| {
                RuntimeClientError::new(format!("session not found: {}", session_id))
            })?;

        let replay_feedback = ReplayFeedback {
            used_capsule: request.used_capsule,
            capsule_id: request.capsule_id,
            planner_directive: if request.used_capsule {
                ReplayPlannerDirective::SkipPlanner
            } else {
                ReplayPlannerDirective::PlanFallback
            },
            reasoning_steps_avoided: if request.used_capsule { 1 } else { 0 },
            fallback_reason: request.fallback_reason,
            task_class_id: request.task_class_id,
            task_label: request.task_label,
            summary: request.summary.clone(),
        };

        let result = A2aTaskSessionResult {
            terminal_state: request.terminal_state,
            summary: request.summary.clone(),
            retryable: false,
            retry_after_ms: None,
            failure_code: None,
            failure_details: None,
            replay_feedback,
        };

        let response = A2aTaskSessionCompletionResponse {
            ack: session,
            result,
        };

        state
            .completions
            .insert(session_id.to_string(), response.clone());

        Ok(response)
    }
}

pub fn default_handshake_request(sender_id: &str) -> A2aHandshakeRequest {
    A2aHandshakeRequest {
        agent_id: sender_id.to_string(),
        role: AgentRole::Planner,
        capability_level: AgentCapabilityLevel::A3,
        supported_protocols: vec![A2aProtocol::current()],
        advertised_capabilities: vec![
            A2aCapability::Coordination,
            A2aCapability::SupervisedDevloop,
            A2aCapability::ReplayFeedback,
        ],
    }
}
