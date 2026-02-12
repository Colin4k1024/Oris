use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

use crate::{
    agent::context_engineering::{ModelRequest, ModelResponse},
    agent::runtime::{Runtime, RuntimeRequest},
    chain::ChainError,
    language_models::GenerateResult,
    prompt::PromptArgs,
    schemas::agent::{AgentAction, AgentEvent, AgentFinish},
};

/// Context information available to middleware during execution.
///
/// This provides stateful information that middleware can use to make decisions
/// and track execution across multiple hooks.
#[derive(Clone, Debug)]
pub struct MiddlewareContext {
    /// Current iteration number in the agent loop
    pub iteration: usize,
    /// Start time of the agent execution
    pub start_time: std::time::Instant,
    /// Total number of tool calls made so far
    pub tool_call_count: usize,
    /// Custom data that middleware can store and retrieve
    pub custom_data: HashMap<String, Value>,
}

impl MiddlewareContext {
    pub fn new() -> Self {
        Self {
            iteration: 0,
            start_time: std::time::Instant::now(),
            tool_call_count: 0,
            custom_data: HashMap::new(),
        }
    }

    pub fn with_iteration(mut self, iteration: usize) -> Self {
        self.iteration = iteration;
        self
    }

    pub fn increment_iteration(&mut self) {
        self.iteration += 1;
    }

    pub fn increment_tool_call_count(&mut self) {
        self.tool_call_count += 1;
    }

    pub fn get_custom_data(&self, key: &str) -> Option<&Value> {
        self.custom_data.get(key)
    }

    pub fn set_custom_data(&mut self, key: String, value: Value) {
        self.custom_data.insert(key, value);
    }
}

impl Default for MiddlewareContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur in middleware execution.
#[derive(Debug, Error)]
pub enum MiddlewareError {
    #[error("Middleware execution error: {0}")]
    ExecutionError(String),

    #[error("Middleware aborted execution: {0}")]
    Aborted(String),

    #[error("Middleware validation error: {0}")]
    ValidationError(String),

    #[error("Chain error: {0}")]
    ChainError(#[from] ChainError),

    /// Human-in-the-loop: pause and return interrupt payload (action_requests, review_configs).
    /// Executor should save state and return result with __interrupt__.
    #[error("Interrupt (human-in-the-loop)")]
    Interrupt(serde_json::Value),

    /// Human rejected this tool call; executor should skip execution and inject a fixed observation.
    #[error("Tool call rejected by user")]
    RejectTool,
}

/// Trait for middleware that can intercept and modify agent execution.
///
/// Middleware provides hooks at various points in the agent execution loop,
/// allowing you to log, monitor, transform, or control agent behavior.
///
/// # Hook Execution Order
///
/// 1. `before_agent_plan` - Called before agent planning
/// 2. `after_agent_plan` - Called after agent planning
/// 3. `before_tool_call` - Called before each tool execution
/// 4. `after_tool_call` - Called after each tool execution
/// 5. `before_finish` - Called before returning final result
/// 6. `after_finish` - Called after returning final result
///
/// # Return Values
///
/// - `Ok(None)`: Continue with original value (no modification)
/// - `Ok(Some(value))`: Replace with new value
/// - `Err(MiddlewareError)`: Abort execution or handle error
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called before the agent plans its next action.
    ///
    /// Can modify the input prompt arguments before they're passed to the agent.
    /// Return `Ok(None)` to use original input, or `Ok(Some(modified_input))` to replace it.
    async fn before_agent_plan(
        &self,
        input: &PromptArgs,
        steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        let _ = (input, steps, context);
        Ok(None)
    }

    /// Called after the agent plans its next action.
    ///
    /// Can modify the agent event (action or finish) before it's processed.
    /// Return `Ok(None)` to use original event, or `Ok(Some(modified_event))` to replace it.
    async fn after_agent_plan(
        &self,
        input: &PromptArgs,
        event: &AgentEvent,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentEvent>, MiddlewareError> {
        let _ = (input, event, context);
        Ok(None)
    }

    /// Called before a tool is executed.
    ///
    /// Can modify the tool action before execution.
    /// Return `Ok(None)` to use original action, or `Ok(Some(modified_action))` to replace it.
    async fn before_tool_call(
        &self,
        action: &AgentAction,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentAction>, MiddlewareError> {
        let _ = (action, context);
        Ok(None)
    }

    /// Called after a tool is executed.
    ///
    /// Can modify the tool observation before it's added to steps.
    /// Return `Ok(None)` to use original observation, or `Ok(Some(modified_observation))` to replace it.
    async fn after_tool_call(
        &self,
        action: &AgentAction,
        observation: &str,
        context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        let _ = (action, observation, context);
        Ok(None)
    }

    /// Called before the agent finishes and returns a result.
    ///
    /// Can modify the finish result before it's returned.
    /// Return `Ok(None)` to use original finish, or `Ok(Some(modified_finish))` to replace it.
    async fn before_finish(
        &self,
        finish: &AgentFinish,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentFinish>, MiddlewareError> {
        let _ = (finish, context);
        Ok(None)
    }

    /// Called after the agent finishes and returns a result.
    ///
    /// This is the final hook for cleanup, logging, or final processing.
    /// Cannot modify the result at this point.
    async fn after_finish(
        &self,
        finish: &AgentFinish,
        result: &GenerateResult,
        context: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        let _ = (finish, result, context);
        Ok(())
    }

    // ===== Runtime-aware hooks (optional, for middleware that needs runtime access) =====

    /// Called before the agent plans its next action (with runtime access).
    ///
    /// This is an optional hook that provides access to runtime information.
    /// Default implementation calls the non-runtime version for backward compatibility.
    async fn before_agent_plan_with_runtime(
        &self,
        request: &RuntimeRequest,
        steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        // Default implementation calls the non-runtime version
        self.before_agent_plan(&request.input, steps, context).await
    }

    /// Called after the agent plans its next action (with runtime access).
    async fn after_agent_plan_with_runtime(
        &self,
        request: &RuntimeRequest,
        event: &AgentEvent,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentEvent>, MiddlewareError> {
        // Default implementation calls the non-runtime version
        self.after_agent_plan(&request.input, event, context).await
    }

    /// Called before a tool is executed (with runtime access).
    async fn before_tool_call_with_runtime(
        &self,
        action: &AgentAction,
        _runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentAction>, MiddlewareError> {
        // Default implementation calls the non-runtime version
        self.before_tool_call(action, context).await
    }

    /// Called after a tool is executed (with runtime access).
    async fn after_tool_call_with_runtime(
        &self,
        action: &AgentAction,
        observation: &str,
        _runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        // Default implementation calls the non-runtime version
        self.after_tool_call(action, observation, context).await
    }

    /// Called before the agent finishes (with runtime access).
    async fn before_finish_with_runtime(
        &self,
        finish: &AgentFinish,
        _runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentFinish>, MiddlewareError> {
        // Default implementation calls the non-runtime version
        self.before_finish(finish, context).await
    }

    /// Called after the agent finishes (with runtime access).
    async fn after_finish_with_runtime(
        &self,
        finish: &AgentFinish,
        result: &GenerateResult,
        _runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        // Default implementation calls the non-runtime version
        self.after_finish(finish, result, context).await
    }

    // ===== Model call hooks (for context engineering) =====

    /// Called before a model call is made.
    ///
    /// This hook allows middleware to modify the model request, including:
    /// - Messages (inject context, modify history)
    /// - Tools (filter or add tools)
    /// - Model (switch to different model)
    /// - Response format (change output schema)
    ///
    /// Return `Ok(None)` to use original request, or `Ok(Some(modified_request))` to replace it.
    async fn before_model_call(
        &self,
        request: &ModelRequest,
        context: &mut MiddlewareContext,
    ) -> Result<Option<ModelRequest>, MiddlewareError> {
        let _ = (request, context);
        Ok(None)
    }

    /// Called after a model call is made.
    ///
    /// This hook allows middleware to modify the model response or add metadata.
    /// Return `Ok(None)` to use original response, or `Ok(Some(modified_response))` to replace it.
    async fn after_model_call(
        &self,
        request: &ModelRequest,
        response: &ModelResponse,
        context: &mut MiddlewareContext,
    ) -> Result<Option<ModelResponse>, MiddlewareError> {
        let _ = (request, response, context);
        Ok(None)
    }
}

pub mod content_filter;
pub mod guardrail_utils;
pub mod human_in_loop;
pub mod logging;
pub mod pii;
pub mod pii_detector;
pub mod rate_limit;
pub mod retry;
pub mod safety_guardrail;
pub mod skill_injection;
pub mod summarization;
pub mod tool_result_eviction;

pub use content_filter::ContentFilterMiddleware;
pub use guardrail_utils::*;
pub use human_in_loop::HumanInTheLoopMiddleware;
pub use logging::{LogLevel, LoggingMiddleware};
pub use pii::{PIIMiddleware, PIIStrategy};
pub use pii_detector::{detect_all_pii, PIIDetector, PIIMatch, PIIType};
pub use rate_limit::RateLimitMiddleware;
pub use retry::RetryMiddleware;
pub use safety_guardrail::SafetyGuardrailMiddleware;
pub use skill_injection::{build_skills_middleware, SkillsMiddleware};
pub use summarization::SummarizationMiddleware;
pub use tool_result_eviction::ToolResultEvictionMiddleware;

// Re-export middleware chain executor
pub mod chain;
pub use chain::{MiddlewareChainConfig, MiddlewareChainExecutor, MiddlewareResult};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_middleware_context() {
        let mut ctx = MiddlewareContext::new();
        assert_eq!(ctx.iteration, 0);
        assert_eq!(ctx.tool_call_count, 0);

        ctx.increment_iteration();
        ctx.increment_tool_call_count();
        assert_eq!(ctx.iteration, 1);
        assert_eq!(ctx.tool_call_count, 1);

        ctx.set_custom_data("key".to_string(), Value::String("value".to_string()));
        assert_eq!(
            ctx.get_custom_data("key"),
            Some(&Value::String("value".to_string()))
        );
    }
}
