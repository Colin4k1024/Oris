//! Task tool: single entry point that delegates to subagents by id or description.
//!
//! See [Subagents](https://docs.langchain.com/oss/python/deepagents/subagents). Use `subagent_id`
//! to choose a subagent (e.g. `"general-purpose"` for the built-in one) and `input` or
//! `task_description` for the task.

use std::collections::HashMap;
use std::error::Error;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::SubagentTool;
use crate::tools::{Tool, ToolResult, ToolRuntime};

/// Single "task" tool that routes to one of several subagents by subagent_id or by description.
/// When no subagent_id is given, the first subagent is used.
pub struct TaskTool {
    subagents: Vec<SubagentTool>,
    name_to_index: HashMap<String, usize>,
}

impl TaskTool {
    /// Build from subagent specs: each (name, description, agent) becomes one option.
    pub fn from_subagent_tools(tools: Vec<SubagentTool>) -> Self {
        let name_to_index: HashMap<String, usize> = tools
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name().to_string(), i))
            .collect();
        Self {
            subagents: tools,
            name_to_index,
        }
    }

    /// One subagent (name + tool).
    pub fn subagents(&self) -> &[SubagentTool] {
        &self.subagents
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn name(&self) -> String {
        "task".to_string()
    }

    fn description(&self) -> String {
        let names: Vec<String> = self.subagents.iter().map(|t| t.name()).collect();
        format!(
            "Delegate a subtask to a specialized subagent. Use 'subagent_id' to choose one of: [{}]. \
             Pass the task description in 'input' or 'task_description'.",
            names.join(", ")
        )
    }

    fn parameters(&self) -> Value {
        let names: Vec<Value> = self.subagents.iter().map(|t| json!(t.name())).collect();
        json!({
            "type": "object",
            "properties": {
                "subagent_id": {
                    "type": "string",
                    "description": "Id/name of the subagent to use",
                    "enum": names
                },
                "input": {
                    "type": "string",
                    "description": "Task input or question for the subagent"
                },
                "task_description": {
                    "type": "string",
                    "description": "Same as input: task description for the subagent"
                }
            },
            "required": ["input"]
        })
    }

    async fn run(&self, _input: Value) -> Result<String, crate::error::ToolError> {
        Err(crate::error::ToolError::ConfigurationError(
            "task tool requires runtime. Use run_with_runtime.".to_string(),
        ))
    }

    async fn run_with_runtime(
        &self,
        input: Value,
        runtime: &ToolRuntime,
    ) -> Result<ToolResult, Box<dyn Error>> {
        let subagent_id = input
            .get("subagent_id")
            .and_then(Value::as_str)
            .map(String::from);

        let task_input = input
            .get("input")
            .or_else(|| input.get("task_description"))
            .and_then(|v| {
                if v.is_string() {
                    v.as_str().map(String::from)
                } else {
                    Some(v.to_string())
                }
            })
            .unwrap_or_else(|| input.to_string());

        let index = if let Some(ref id) = subagent_id {
            self.name_to_index.get(id).copied()
        } else {
            None
        };

        let tool = index
            .and_then(|i| self.subagents.get(i))
            .or_else(|| self.subagents.first())
            .ok_or("task tool has no subagents configured")?;

        let input_value = json!({ "input": task_input });
        tool.run_with_runtime(input_value, runtime).await
    }

    fn requires_runtime(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::agent::create_agent;

    #[test]
    fn test_task_tool_from_subagents() {
        let agent = Arc::new(create_agent("gpt-4o-mini", &[], Some("Test"), None).unwrap());
        let t1 = SubagentTool::new(
            Arc::clone(&agent),
            "researcher".to_string(),
            "Research".to_string(),
        );
        let t2 = SubagentTool::new(agent, "coder".to_string(), "Code".to_string());
        let task = TaskTool::from_subagent_tools(vec![t1, t2]);
        assert_eq!(task.name(), "task");
        assert_eq!(task.subagents().len(), 2);
        assert!(task.requires_runtime());
    }
}
