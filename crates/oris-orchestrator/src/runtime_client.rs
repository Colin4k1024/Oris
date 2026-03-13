use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use oris_agent_contract::{
    infer_replay_fallback_reason_code, normalize_replay_fallback_contract, A2aCapability,
    A2aHandshakeRequest, A2aHandshakeResponse, A2aProtocol, A2aTaskLifecycleState,
    A2aTaskSessionAck, A2aTaskSessionCompletionRequest, A2aTaskSessionCompletionResponse,
    A2aTaskSessionResult, A2aTaskSessionStartRequest, AgentCapabilityLevel, AgentRole,
    ReplayFallbackNextAction, ReplayFallbackReasonCode, ReplayFeedback, ReplayPlannerDirective,
    A2A_TASK_SESSION_PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json;

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
    pub reason_code: Option<ReplayFallbackReasonCode>,
    pub repair_hint: Option<String>,
    pub next_action: Option<ReplayFallbackNextAction>,
    pub confidence: Option<u8>,
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
            reason_code: None,
            repair_hint: None,
            next_action: None,
            confidence: None,
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
            reason_code: self.reason_code,
            repair_hint: self.repair_hint,
            next_action: self.next_action,
            confidence: self.confidence,
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

        let planner_directive = if request.used_capsule {
            ReplayPlannerDirective::SkipPlanner
        } else {
            ReplayPlannerDirective::PlanFallback
        };
        let reason_code_hint = request.reason_code.or_else(|| {
            request
                .fallback_reason
                .as_deref()
                .and_then(infer_replay_fallback_reason_code)
        });
        let fallback_contract = normalize_replay_fallback_contract(
            &planner_directive,
            request.fallback_reason.as_deref(),
            reason_code_hint,
            request.repair_hint.as_deref(),
            request.next_action,
            request.confidence,
        );

        let replay_feedback = ReplayFeedback {
            used_capsule: request.used_capsule,
            capsule_id: request.capsule_id,
            planner_directive,
            reasoning_steps_avoided: if request.used_capsule { 1 } else { 0 },
            fallback_reason: fallback_contract
                .as_ref()
                .map(|contract| contract.fallback_reason.clone()),
            reason_code: fallback_contract
                .as_ref()
                .map(|contract| contract.reason_code),
            repair_hint: fallback_contract
                .as_ref()
                .map(|contract| contract.repair_hint.clone()),
            next_action: fallback_contract
                .as_ref()
                .map(|contract| contract.next_action),
            confidence: fallback_contract
                .as_ref()
                .map(|contract| contract.confidence),
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

use oris_agent_contract::{HubOperationClass, HubProfile, HubSelectionPolicy, HubTrustTier};

/// Multi-hub runtime client that routes A2A operations based on hub trust tiers.
///
/// This client supports:
/// - Internal/private hubs with Full trust (all operations)
/// - Public hubs with ReadOnly trust (read-only operations: Hello, Fetch)
/// - Automatic fallback: if internal hub fails for Fetch, try public hub
#[derive(Clone)]
pub struct MultiHubRuntimeA2aClient {
    hubs: Vec<HubProfile>,
    policy: HubSelectionPolicy,
    http_client: reqwest::Client,
}

impl MultiHubRuntimeA2aClient {
    /// Create a new multi-hub client with the given hub profiles
    pub fn new(hubs: Vec<HubProfile>) -> Self {
        Self {
            hubs,
            policy: HubSelectionPolicy::default(),
            http_client: reqwest::Client::new(),
        }
    }

    /// Create a new multi-hub client with custom selection policy
    pub fn with_policy(hubs: Vec<HubProfile>, policy: HubSelectionPolicy) -> Self {
        Self {
            hubs,
            policy,
            http_client: reqwest::Client::new(),
        }
    }

    /// Select a hub for the given operation class
    fn select_hub(&self, operation: &HubOperationClass) -> Option<&HubProfile> {
        let allowed_tiers = self.policy.allowed_tiers(operation);

        // Filter hubs by allowed trust tiers and sort by priority (descending)
        let mut candidates: Vec<&HubProfile> = self
            .hubs
            .iter()
            .filter(|hub| {
                allowed_tiers.contains(&hub.trust_tier) && hub.allows_operation(operation)
            })
            .collect();

        // Sort by priority descending
        candidates.sort_by(|a, b| b.priority.cmp(&a.priority));

        candidates.first().copied()
    }

    /// Execute an operation on the selected hub, with fallback for read-only operations
    async fn execute<TReq, TResp>(
        &self,
        operation: HubOperationClass,
        path: &str,
        request: &TReq,
    ) -> Result<TResp, RuntimeClientError>
    where
        TReq: serde::Serialize,
        TResp: serde::de::DeserializeOwned,
    {
        // Try internal hub first
        if let Some(hub) = self.select_hub(&operation) {
            match self.execute_on_hub(hub, path, request).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    // For read-only operations, try fallback to public hub
                    if operation.is_read_only() {
                        let fallback_hub = self.fallback_hub(hub, &operation);
                        if let Some(public_hub) = fallback_hub {
                            // Need to re-serialize the request for the fallback hub
                            let resp = self.execute_on_hub(&public_hub, path, request).await?;
                            return Ok(resp);
                        }
                    }
                    return Err(e);
                }
            }
        }

        Err(RuntimeClientError::new(format!(
            "no hub available for operation {:?}",
            operation
        )))
    }

    /// Find a fallback hub (public/readonly) when internal hub fails
    fn fallback_hub<'a>(
        &'a self,
        excluded: &'a HubProfile,
        operation: &HubOperationClass,
    ) -> Option<&'a HubProfile> {
        self.hubs
            .iter()
            .filter(|hub| {
                hub.hub_id != excluded.hub_id
                    && hub.trust_tier == HubTrustTier::ReadOnly
                    && hub.allows_operation(operation)
            })
            .max_by_key(|hub| hub.priority)
    }

    /// Execute a request on a specific hub
    async fn execute_on_hub<TReq, TResp>(
        &self,
        hub: &HubProfile,
        path: &str,
        request: &TReq,
    ) -> Result<TResp, RuntimeClientError>
    where
        TReq: serde::Serialize,
        TResp: serde::de::DeserializeOwned,
    {
        let url = format!(
            "{}/{}",
            hub.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        let response = self.http_client.post(url).json(request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeClientError::new(format!(
                "hub {} returned {}: {}",
                hub.hub_id, status, body
            )));
        }

        let envelope = response.json::<ApiEnvelope<TResp>>().await?;

        // Add source tracking to response if the type supports it
        Ok(envelope.data)
    }

    /// Perform handshake on the best available hub
    pub async fn handshake(
        &self,
        request: A2aHandshakeRequest,
    ) -> Result<A2aHandshakeResponse, RuntimeClientError> {
        self.execute(HubOperationClass::Hello, "/a2a/hello", &request)
            .await
    }

    /// Fetch assets/tasks from the best available hub (with fallback to public)
    pub async fn fetch(
        &self,
        request: impl serde::Serialize,
    ) -> Result<serde_json::Value, RuntimeClientError> {
        self.execute(HubOperationClass::Fetch, "/a2a/fetch", &request)
            .await
    }

    /// Publish an asset (requires full-trust hub)
    pub async fn publish(
        &self,
        request: impl serde::Serialize,
    ) -> Result<serde_json::Value, RuntimeClientError> {
        self.execute(HubOperationClass::Publish, "/a2a/publish", &request)
            .await
    }

    /// Claim a task (requires full-trust hub)
    pub async fn claim_task(
        &self,
        request: impl serde::Serialize,
    ) -> Result<serde_json::Value, RuntimeClientError> {
        self.execute(HubOperationClass::TaskClaim, "/a2a/task/claim", &request)
            .await
    }

    /// Complete a task (requires full-trust hub)
    pub async fn complete_task(
        &self,
        request: impl serde::Serialize,
    ) -> Result<serde_json::Value, RuntimeClientError> {
        self.execute(
            HubOperationClass::TaskComplete,
            "/a2a/task/complete",
            &request,
        )
        .await
    }

    /// Send heartbeat (requires full-trust hub)
    pub async fn heartbeat(
        &self,
        request: impl serde::Serialize,
    ) -> Result<serde_json::Value, RuntimeClientError> {
        self.execute(HubOperationClass::Hello, "/a2a/heartbeat", &request)
            .await
    }
}

/// Builder for creating MultiHubRuntimeA2aClient with fluent configuration
pub struct MultiHubRuntimeA2aClientBuilder {
    hubs: Vec<HubProfile>,
    policy: Option<HubSelectionPolicy>,
}

impl MultiHubRuntimeA2aClientBuilder {
    pub fn new() -> Self {
        Self {
            hubs: Vec::new(),
            policy: None,
        }
    }

    /// Add an internal/private hub with full trust
    pub fn with_internal_hub(
        mut self,
        hub_id: impl Into<String>,
        base_url: impl Into<String>,
        priority: u32,
    ) -> Self {
        self.hubs.push(HubProfile {
            hub_id: hub_id.into(),
            base_url: base_url.into(),
            trust_tier: HubTrustTier::Full,
            priority,
            health_url: None,
        });
        self
    }

    /// Add a public hub with read-only trust
    pub fn with_public_hub(
        mut self,
        hub_id: impl Into<String>,
        base_url: impl Into<String>,
        priority: u32,
    ) -> Self {
        self.hubs.push(HubProfile {
            hub_id: hub_id.into(),
            base_url: base_url.into(),
            trust_tier: HubTrustTier::ReadOnly,
            priority,
            health_url: None,
        });
        self
    }

    /// Set custom selection policy
    pub fn with_policy(mut self, policy: HubSelectionPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Build the multi-hub client
    pub fn build(self) -> MultiHubRuntimeA2aClient {
        MultiHubRuntimeA2aClient::with_policy(self.hubs, self.policy.unwrap_or_default())
    }
}

impl Default for MultiHubRuntimeA2aClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
