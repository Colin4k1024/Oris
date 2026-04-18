use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

use crate::error::ToolError;
use crate::tools::Tool;

/// Allowlist of permitted commands for sandboxed execution
const ALLOWED_COMMANDS: &[&str] = &[
    "ls", "cat", "echo", "pwd", "cd", "mkdir", "rmdir", "touch", "rm",
    "cp", "mv", "head", "tail", "wc", "grep", "find", "sort", "uniq",
    "git", "cargo", "rustc", "md5sum", "sha256sum",
];

pub struct CommandExecutor {
    platform: String,
    allowed_commands: HashSet<String>,
}

impl CommandExecutor {
    /// Create a new CommandExecutor instance with default allowlist
    /// # Example
    /// ```rust,ignore
    /// let tool = CommandExecutor::new("linux");
    /// ```
    pub fn new<S: Into<String>>(platform: S) -> Self {
        Self {
            platform: platform.into(),
            allowed_commands: ALLOWED_COMMANDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Create a new CommandExecutor with a custom allowlist (for testing/admin purposes)
    /// This is intentionally not pub - use `new()` for normal use
    fn new_with_allowlist<S: Into<String>>(platform: S, allowlist: HashSet<String>) -> Self {
        Self {
            platform: platform.into(),
            allowed_commands: allowlist,
        }
    }

    /// Validate a command against the allowlist
    fn validate_command(&self, cmd: &str) -> Result<(), ToolError> {
        let cmd_name = cmd.split_whitespace().next().unwrap_or(cmd);
        if !self.allowed_commands.contains(cmd_name) {
            return Err(ToolError::ExecutionError(format!(
                "Command '{}' not allowed. Allowed commands: {:?}",
                cmd_name, self.allowed_commands
            )));
        }
        Ok(())
    }

    /// Execute commands with allowlist validation (sandboxed execution)
    async fn execute_sandboxed(&self, input: Value) -> Result<String, ToolError> {
        let commands: Vec<CommandInput> = serde_json::from_value(input)
            .map_err(|e| ToolError::ParsingError(e.to_string()))?;
        let mut result = String::new();

        for command in commands {
            // Validate command against allowlist
            self.validate_command(&command.cmd)?;

            let mut command_to_execute = std::process::Command::new(&command.cmd);
            command_to_execute.args(&command.args);

            let output = command_to_execute
                .output()
                .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

            result.push_str(&format!(
                "Command: {}\nOutput: {}",
                command.cmd,
                String::from_utf8_lossy(&output.stdout),
            ));

            if !output.status.success() {
                return Err(ToolError::ExecutionError(format!(
                    "Command {} failed with status: {}",
                    command.cmd, output.status
                )));
            }
        }

        Ok(result)
    }
}

impl Default for CommandExecutor {
    fn default() -> Self {
        Self::new("linux")
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct CommandInput {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
}
#[derive(Serialize, Deserialize, Debug)]
struct CommandsWrapper {
    commands: Vec<CommandInput>,
}

#[async_trait]
impl Tool for CommandExecutor {
    fn name(&self) -> String {
        String::from("Command_Executor")
    }
    fn description(&self) -> String {
        format!(
            r#""This tool let you run command on the terminal"
            "The input should be an array with commands for the following platform: {}"
            "examle of input: [{{ "cmd": "ls", "args": [] }},{{"cmd":"mkdir","args":["test"]}}]"
            "Should be a comma separated commands"
            "#,
            self.platform
        )
    }

    fn parameters(&self) -> Value {
        let prompt = format!(
            "This tool let you run command on the terminal.
        The input should be an array with commands for the following platform: {}",
            self.platform
        );
        json!(

        {
          "description": prompt,
          "type": "object",
          "properties": {
            "commands": {
              "description": "An array of command objects to be executed",
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "cmd": {
                    "type": "string",
                    "description": "The command to execute"
                  },
                  "args": {
                    "type": "array",
                    "items": {
                      "type": "string"
                    },
                    "default": [],
                    "description": "List of arguments for the command"
                  }
                },
                "required": ["cmd"],
                "additionalProperties": false,
                "description": "Object representing a command and its optional arguments"
              }
            }
          },
          "required": ["commands"],
          "additionalProperties": false
        }
                )
    }

    async fn parse_input(&self, input: &str) -> Value {
        log::info!("Parsing input: {}", input);

        // Attempt to parse input string into CommandsWrapper struct first
        let wrapper_result = serde_json::from_str::<CommandsWrapper>(input);

        if let Ok(wrapper) = wrapper_result {
            // If successful, serialize the `commands` back into a serde_json::Value
            // this is for llm like open ai tools
            serde_json::to_value(wrapper.commands).unwrap_or_else(|err| {
                log::error!("Serialization error: {}", err);
                Value::Null
            })
        } else {
            // If the first attempt fails, try parsing it as Vec<CommandInput> directly
            // This works on any llm
            let commands_result = serde_json::from_str::<Vec<CommandInput>>(input);

            commands_result.map_or_else(
                |err| {
                    log::error!("Failed to parse input: {}", err);
                    Value::Null
                },
                |commands| serde_json::to_value(commands).unwrap_or(Value::Null),
            )
        }
    }

    async fn run(&self, input: Value) -> Result<String, crate::error::ToolError> {
        self.execute_sandboxed(input).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn test_with_string_executor() {
        let tool = CommandExecutor::new("linux");
        let input = json!({
            "commands": [
                {
                    "cmd": "ls",
                    "args": []
                }
            ]
        });
        println!("{}", &input.to_string());
        let result = tool.call(&input.to_string()).await.unwrap();
        println!("Res: {}", result);
    }
}
