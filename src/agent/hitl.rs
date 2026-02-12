//! Human-in-the-loop (HILP) types for deep agent.
//!
//! Aligned with [Human-in-the-loop](https://docs.langchain.com/oss/python/deepagents/human-in-the-loop):
//! interrupt payload (action_requests, review_configs), resume with decisions (approve, edit, reject).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default allowed decisions when interrupt is enabled with no custom config.
pub const DEFAULT_ALLOWED_DECISIONS: &[&str] = &["approve", "edit", "reject"];

/// Human decision for a pending tool call.
///
/// Matches Python: `{"type": "approve"}`, `{"type": "edit", "edited_action": {...}}`, `{"type": "reject"}`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HitlDecision {
    /// Execute the tool with the original arguments.
    Approve,

    /// Execute with modified arguments. Must include tool name and args.
    Edit {
        /// Tool name (must match action_request).
        #[serde(rename = "edited_action")]
        edited_action: EditedAction,
    },

    /// Skip executing this tool call.
    Reject,
}

/// Edited tool action for an "edit" decision.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditedAction {
    /// Tool name.
    pub name: String,
    /// Tool arguments (JSON object as string or map).
    #[serde(alias = "args")]
    pub args: Value,
}

/// Per-tool interrupt configuration.
///
/// Can be enabled with default decisions (`["approve", "edit", "reject"]`) or with a custom
/// subset (e.g. `["approve", "reject"]` only).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptConfig {
    /// Whether to interrupt before this tool.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Allowed human decisions. Default: ["approve", "edit", "reject"].
    #[serde(default = "default_allowed_decisions", rename = "allowed_decisions")]
    pub allowed_decisions: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_allowed_decisions() -> Vec<String> {
    DEFAULT_ALLOWED_DECISIONS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

impl Default for InterruptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_decisions: default_allowed_decisions(),
        }
    }
}

impl InterruptConfig {
    /// Enable interrupt with default decisions (approve, edit, reject).
    pub fn enabled() -> Self {
        Self::default()
    }

    /// Disable interrupt for this tool.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            allowed_decisions: default_allowed_decisions(),
        }
    }

    /// Enable with specific allowed decisions.
    pub fn with_allowed_decisions(mut self, decisions: Vec<String>) -> Self {
        self.allowed_decisions = decisions;
        self
    }

    /// Parse from a config value: `true` | `false` | `{"allowed_decisions": [...]}`.
    pub fn from_value(v: &Value) -> Option<Self> {
        if let Some(b) = v.as_bool() {
            return Some(if b { Self::enabled() } else { Self::disabled() });
        }
        if let Some(obj) = v.as_object() {
            let enabled = obj.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
            let allowed_decisions = obj
                .get("allowed_decisions")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_else(default_allowed_decisions);
            return Some(Self {
                enabled,
                allowed_decisions,
            });
        }
        None
    }
}

/// One action pending human review (tool name + args).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionRequest {
    /// Tool name.
    pub name: String,
    /// Tool arguments (typically a JSON object).
    pub args: Value,
}

/// Review config for one action (allowed decisions).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewConfig {
    /// Tool/action name.
    #[serde(rename = "action_name")]
    pub action_name: String,
    /// Allowed decision types.
    #[serde(rename = "allowed_decisions")]
    pub allowed_decisions: Vec<String>,
}

/// Interrupt payload included in `__interrupt__`.
///
/// Returned when execution pauses for human approval; caller uses this to display
/// pending actions and then resume with `Command::resume({ "decisions": [...] })`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptPayload {
    /// Pending tool calls (name + args), in order.
    #[serde(rename = "action_requests")]
    pub action_requests: Vec<ActionRequest>,

    /// Per-action allowed decisions, same order as action_requests.
    #[serde(rename = "review_configs")]
    pub review_configs: Vec<ReviewConfig>,
}

/// Resume payload: `{ "decisions": [ HitlDecision, ... ] }`.
///
/// One decision per action_request, in the same order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResumePayload {
    /// Human decisions for each pending action.
    pub decisions: Vec<HitlDecision>,
}

impl ResumePayload {
    /// Parse from the value passed to `Command::resume(value)`.
    pub fn from_value(v: &Value) -> Option<Self> {
        let decisions = v.get("decisions")?.as_array()?;
        let decisions: Vec<HitlDecision> = decisions
            .iter()
            .filter_map(|d| serde_json::from_value(d.clone()).ok())
            .collect();
        Some(Self { decisions })
    }
}

/// Result of agent invocation when using [crate::agent::UnifiedAgent::invoke_with_config].
/// Either completion with output string or interrupt with action_requests/review_configs for HILP.
#[derive(Clone, Debug)]
pub enum AgentInvokeResult {
    /// Agent completed; output is the final generation.
    Complete(String),
    /// Execution was interrupted for human approval; use `interrupt_value` (action_requests, review_configs) and resume with [crate::langgraph::Command::resume].
    Interrupt {
        /// Interrupt payload (action_requests, review_configs). Expose as `__interrupt__` in JSON.
        interrupt_value: serde_json::Value,
    },
}

impl AgentInvokeResult {
    /// Whether this result is an interrupt (requires resume with decisions).
    pub fn is_interrupt(&self) -> bool {
        matches!(self, AgentInvokeResult::Interrupt { .. })
    }

    /// Convert to JSON (state key + optional `__interrupt__`), similar to LangGraph InvokeResult.
    pub fn to_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            AgentInvokeResult::Complete(s) => Ok(serde_json::json!({ "output": s })),
            AgentInvokeResult::Interrupt { interrupt_value } => {
                Ok(serde_json::json!({ "__interrupt__": [ { "value": interrupt_value } ] }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interrupt_config_default() {
        let c = InterruptConfig::default();
        assert!(c.enabled);
        assert_eq!(c.allowed_decisions, vec!["approve", "edit", "reject"]);
    }

    #[test]
    fn test_interrupt_config_from_value_bool() {
        let c = InterruptConfig::from_value(&serde_json::json!(true)).unwrap();
        assert!(c.enabled);
        let c = InterruptConfig::from_value(&serde_json::json!(false)).unwrap();
        assert!(!c.enabled);
    }

    #[test]
    fn test_interrupt_config_from_value_object() {
        let c = InterruptConfig::from_value(&serde_json::json!({
            "allowed_decisions": ["approve", "reject"]
        }))
        .unwrap();
        assert!(c.enabled);
        assert_eq!(c.allowed_decisions, vec!["approve", "reject"]);
    }

    #[test]
    fn test_resume_payload_from_value() {
        let v = serde_json::json!({
            "decisions": [
                {"type": "approve"},
                {"type": "reject"}
            ]
        });
        let p = ResumePayload::from_value(&v).unwrap();
        assert_eq!(p.decisions.len(), 2);
        assert!(matches!(p.decisions[0], HitlDecision::Approve));
        assert!(matches!(p.decisions[1], HitlDecision::Reject));
    }
}
