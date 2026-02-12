use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::agent::hitl::{ActionRequest, InterruptConfig, InterruptPayload, ReviewConfig};
use crate::schemas::agent::{AgentAction, AgentFinish};

/// Key in MiddlewareContext for pending decisions on resume (serde Value array of HitlDecision).
pub const RESUME_DECISIONS_KEY: &str = "resume_decisions";
/// Key for current index into resume_decisions (number).
pub const RESUME_DECISION_INDEX_KEY: &str = "resume_decision_index";
/// Key for current batch of actions (array of AgentAction), set by executor before tool loop.
pub const CURRENT_BATCH_ACTIONS_KEY: &str = "current_batch_actions";

/// Human-in-the-loop middleware for requiring human approval.
///
/// Supports interrupt/resume mode: when approval is required and no "resume_decisions" are in
/// context, returns [MiddlewareError::Interrupt] with action_requests and review_configs.
/// On resume, executor injects "resume_decisions"; middleware consumes one per tool (Approve,
/// Edit, Reject). Reject yields [MiddlewareError::RejectTool] so executor can skip the tool.
///
/// Also supports legacy in-place wait via [Self::with_timeout] (placeholder auto-approve).
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::HumanInTheLoopMiddleware;
/// use oris::agent::InterruptConfig;
///
/// let middleware = HumanInTheLoopMiddleware::new()
///     .with_interrupt_on("send_email".to_string(), true)
///     .with_interrupt_config("delete_file", InterruptConfig::with_allowed_decisions(vec!["approve", "reject"].into_iter().map(String::from).collect()))
///     .with_approval_required_for_finish(true);
/// ```
pub struct HumanInTheLoopMiddleware {
    approval_required_for_tool_calls: bool,
    approval_required_for_finish: bool,
    /// Per-tool: true => enabled with default decisions (legacy); InterruptConfig => full config.
    interrupt_on: HashMap<String, InterruptConfig>,
    timeout: Option<Duration>,
    default_on_timeout: bool,
}

impl HumanInTheLoopMiddleware {
    pub fn new() -> Self {
        Self {
            approval_required_for_tool_calls: false,
            approval_required_for_finish: false,
            interrupt_on: HashMap::new(),
            timeout: None,
            default_on_timeout: true,
        }
    }

    pub fn with_approval_required_for_tool_calls(mut self, required: bool) -> Self {
        self.approval_required_for_tool_calls = required;
        self
    }

    pub fn with_approval_required_for_finish(mut self, required: bool) -> Self {
        self.approval_required_for_finish = required;
        self
    }

    /// Configure approval for a tool: `true` = enable with default decisions, `false` = no interrupt.
    pub fn with_interrupt_on(mut self, tool_name: String, interrupt: bool) -> Self {
        self.interrupt_on.insert(
            tool_name,
            if interrupt {
                InterruptConfig::enabled()
            } else {
                InterruptConfig::disabled()
            },
        );
        self
    }

    /// Configure one tool with full [InterruptConfig].
    pub fn with_interrupt_config(
        mut self,
        tool_name: impl Into<String>,
        config: InterruptConfig,
    ) -> Self {
        self.interrupt_on.insert(tool_name.into(), config);
        self
    }

    /// Configure multiple tools (name -> config).
    pub fn with_interrupt_on_map(mut self, map: HashMap<String, InterruptConfig>) -> Self {
        self.interrupt_on.extend(map);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_default_on_timeout(mut self, default: bool) -> Self {
        self.default_on_timeout = default;
        self
    }

    /// Whether this tool requires approval (interrupt or in-place wait).
    pub fn requires_approval_for_tool(&self, tool_name: &str) -> bool {
        self.interrupt_config_for_tool(tool_name)
            .map(|c| c.enabled)
            .unwrap_or(self.approval_required_for_tool_calls)
    }

    /// Per-tool interrupt config if set.
    pub fn interrupt_config_for_tool(&self, tool_name: &str) -> Option<&InterruptConfig> {
        self.interrupt_on.get(tool_name)
    }

    /// Parse tool_input string as JSON for action_requests args.
    fn args_from_tool_input(tool_input: &str) -> serde_json::Value {
        serde_json::from_str(tool_input).unwrap_or_else(|_| serde_json::json!(tool_input))
    }

    async fn wait_for_approval(&self, description: &str) -> Result<bool, MiddlewareError> {
        log::info!("Human approval required for: {}", description);
        log::info!("[Placeholder] Auto-approving (in production, this would wait for user input)");
        if let Some(timeout) = self.timeout {
            tokio::time::sleep(timeout).await;
            Ok(self.default_on_timeout)
        } else {
            Ok(true)
        }
    }
}

impl Default for HumanInTheLoopMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for HumanInTheLoopMiddleware {
    async fn before_tool_call(
        &self,
        action: &AgentAction,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentAction>, MiddlewareError> {
        let config = self.interrupt_config_for_tool(&action.tool);
        let needs_approval = config
            .map(|c| c.enabled)
            .unwrap_or(self.approval_required_for_tool_calls);

        if !needs_approval {
            return Ok(None);
        }

        // Resume path: consume next decision
        let decisions_value = context.get_custom_data(RESUME_DECISIONS_KEY);
        let index_value = context.get_custom_data(RESUME_DECISION_INDEX_KEY);
        if let (Some(serde_json::Value::Array(decisions)), Some(serde_json::Value::Number(n))) =
            (decisions_value, index_value)
        {
            let idx = n.as_u64().unwrap_or(0) as usize;
            if idx < decisions.len() {
                let decision_value = &decisions[idx];
                if let Ok(decision) =
                    serde_json::from_value::<crate::agent::HitlDecision>(decision_value.clone())
                {
                    context.set_custom_data(
                        RESUME_DECISION_INDEX_KEY.to_string(),
                        serde_json::json!(idx + 1),
                    );
                    return match decision {
                        crate::agent::HitlDecision::Approve => Ok(None),
                        crate::agent::HitlDecision::Edit { edited_action } => {
                            let modified = AgentAction {
                                tool: edited_action.name.clone(),
                                tool_input: serde_json::to_string(&edited_action.args)
                                    .unwrap_or_default(),
                                log: action.log.clone(),
                            };
                            Ok(Some(modified))
                        }
                        crate::agent::HitlDecision::Reject => Err(MiddlewareError::RejectTool),
                    };
                }
            }
        }

        // Interrupt path: build payload for all tools in current batch that need approval
        let batch_actions: Vec<AgentAction> = context
            .get_custom_data(CURRENT_BATCH_ACTIONS_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_else(|| vec![action.clone()]);

        let mut action_requests: Vec<ActionRequest> = Vec::new();
        let mut review_configs: Vec<ReviewConfig> = Vec::new();
        for a in &batch_actions {
            let cfg = self.interrupt_config_for_tool(&a.tool);
            let enabled = cfg
                .map(|c| c.enabled)
                .unwrap_or(self.approval_required_for_tool_calls);
            if enabled {
                action_requests.push(ActionRequest {
                    name: a.tool.clone(),
                    args: Self::args_from_tool_input(&a.tool_input),
                });
                review_configs.push(ReviewConfig {
                    action_name: a.tool.clone(),
                    allowed_decisions: cfg.map(|c| c.allowed_decisions.clone()).unwrap_or_else(
                        || {
                            crate::agent::hitl::DEFAULT_ALLOWED_DECISIONS
                                .iter()
                                .map(|s| (*s).to_string())
                                .collect()
                        },
                    ),
                });
            }
        }

        // If this action is not in the batch we built, it was already approved (e.g. duplicate name)
        if action_requests.is_empty() {
            return Ok(None);
        }

        let payload = InterruptPayload {
            action_requests,
            review_configs,
        };
        Err(MiddlewareError::Interrupt(
            serde_json::to_value(&payload).unwrap_or_default(),
        ))
    }

    async fn before_finish(
        &self,
        finish: &AgentFinish,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentFinish>, MiddlewareError> {
        if self.approval_required_for_finish {
            let description = format!("Final result: {}", finish.output);
            let approved = self.wait_for_approval(&description).await?;
            if !approved {
                return Err(MiddlewareError::Aborted(
                    "Human rejected final result".to_string(),
                ));
            }
            context.set_custom_data("human_approved_finish".to_string(), serde_json::json!(true));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::InterruptConfig;

    #[test]
    fn test_human_in_loop_middleware_creation() {
        let middleware = HumanInTheLoopMiddleware::new();
        assert!(!middleware.approval_required_for_tool_calls);
        assert!(!middleware.approval_required_for_finish);
        assert!(middleware.interrupt_on.is_empty());
    }

    #[test]
    fn test_per_tool_approval() {
        let middleware = HumanInTheLoopMiddleware::new()
            .with_interrupt_on("send_email".to_string(), true)
            .with_interrupt_on("search".to_string(), false);

        assert!(middleware.requires_approval_for_tool("send_email"));
        assert!(!middleware.requires_approval_for_tool("search"));
        assert!(!middleware.requires_approval_for_tool("unknown_tool"));
    }

    #[test]
    fn test_with_interrupt_config() {
        let middleware = HumanInTheLoopMiddleware::new().with_interrupt_config(
            "write_file",
            InterruptConfig::with_allowed_decisions(vec![
                "approve".to_string(),
                "reject".to_string(),
            ]),
        );

        assert!(middleware.requires_approval_for_tool("write_file"));
        let cfg = middleware.interrupt_config_for_tool("write_file").unwrap();
        assert_eq!(cfg.allowed_decisions, vec!["approve", "reject"]);
    }

    #[test]
    fn test_global_vs_per_tool_precedence() {
        let middleware = HumanInTheLoopMiddleware::new()
            .with_approval_required_for_tool_calls(true)
            .with_interrupt_on("search".to_string(), false);

        assert!(!middleware.requires_approval_for_tool("search"));
        assert!(middleware.requires_approval_for_tool("other_tool"));
    }

    #[tokio::test]
    async fn test_wait_for_approval() {
        let middleware = HumanInTheLoopMiddleware::new()
            .with_timeout(Duration::from_millis(10))
            .with_default_on_timeout(true);

        let approved = middleware.wait_for_approval("test action").await.unwrap();
        assert!(approved);
    }
}
