use async_trait::async_trait;
use serde_json::json;

use super::{Middleware, MiddlewareContext, MiddlewareError};
use crate::language_models::GenerateResult;
use crate::prompt::PromptArgs;
use crate::schemas::agent::{AgentAction, AgentEvent, AgentFinish};

/// Logging middleware for tracking agent execution.
///
/// Logs agent planning, tool calls, and results at configurable log levels.
/// Supports both structured JSON logging and human-readable formats.
///
/// # Example
/// ```rust,ignore
/// use oris::agent::middleware::LoggingMiddleware;
///
/// let middleware = LoggingMiddleware::new()
///     .with_log_level(LogLevel::Info)
///     .with_structured_logging(true);
/// ```
pub struct LoggingMiddleware {
    log_level: LogLevel,
    structured: bool,
    log_agent_plan: bool,
    log_tool_calls: bool,
    log_finish: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LoggingMiddleware {
    pub fn new() -> Self {
        Self {
            log_level: LogLevel::Info,
            structured: false,
            log_agent_plan: true,
            log_tool_calls: true,
            log_finish: true,
        }
    }

    pub fn with_log_level(mut self, level: LogLevel) -> Self {
        self.log_level = level;
        self
    }

    pub fn with_structured_logging(mut self, structured: bool) -> Self {
        self.structured = structured;
        self
    }

    pub fn with_log_agent_plan(mut self, log: bool) -> Self {
        self.log_agent_plan = log;
        self
    }

    pub fn with_log_tool_calls(mut self, log: bool) -> Self {
        self.log_tool_calls = log;
        self
    }

    pub fn with_log_finish(mut self, log: bool) -> Self {
        self.log_finish = log;
        self
    }

    fn should_log(&self, level: LogLevel) -> bool {
        match (self.log_level, level) {
            (LogLevel::Debug, _) => true,
            (LogLevel::Info, LogLevel::Info | LogLevel::Warn | LogLevel::Error) => true,
            (LogLevel::Warn, LogLevel::Warn | LogLevel::Error) => true,
            (LogLevel::Error, LogLevel::Error) => true,
            _ => false,
        }
    }

    fn log(&self, level: LogLevel, message: &str, data: Option<serde_json::Value>) {
        if !self.should_log(level) {
            return;
        }

        if self.structured {
            let log_entry = json!({
                "level": format!("{:?}", level),
                "message": message,
                "data": data,
            });
            match level {
                LogLevel::Debug => log::debug!("{}", log_entry),
                LogLevel::Info => log::info!("{}", log_entry),
                LogLevel::Warn => log::warn!("{}", log_entry),
                LogLevel::Error => log::error!("{}", log_entry),
            }
        } else {
            let data_str = data.as_ref().map(|v| format!(" {}", v)).unwrap_or_default();
            match level {
                LogLevel::Debug => log::debug!("{}{}", message, data_str),
                LogLevel::Info => log::info!("{}{}", message, data_str),
                LogLevel::Warn => log::warn!("{}{}", message, data_str),
                LogLevel::Error => log::error!("{}{}", message, data_str),
            }
        }
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_agent_plan(
        &self,
        input: &PromptArgs,
        steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        if self.log_agent_plan {
            self.log(
                LogLevel::Debug,
                "Before agent plan",
                Some(json!({
                    "iteration": context.iteration,
                    "steps_count": steps.len(),
                    "input_keys": input.keys().collect::<Vec<_>>(),
                })),
            );
        }
        Ok(None)
    }

    async fn after_agent_plan(
        &self,
        _input: &PromptArgs,
        event: &AgentEvent,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentEvent>, MiddlewareError> {
        if self.log_agent_plan {
            match event {
                AgentEvent::Action(actions) => {
                    self.log(
                        LogLevel::Info,
                        "Agent planned actions",
                        Some(json!({
                            "iteration": context.iteration,
                            "action_count": actions.len(),
                            "actions": actions.iter().map(|a| &a.tool).collect::<Vec<_>>(),
                        })),
                    );
                }
                AgentEvent::Finish(finish) => {
                    self.log(
                        LogLevel::Info,
                        "Agent finished",
                        Some(json!({
                            "iteration": context.iteration,
                            "output": finish.output,
                        })),
                    );
                }
            }
        }
        Ok(None)
    }

    async fn before_tool_call(
        &self,
        action: &AgentAction,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentAction>, MiddlewareError> {
        if self.log_tool_calls {
            self.log(
                LogLevel::Info,
                "Before tool call",
                Some(json!({
                    "tool": action.tool,
                    "tool_input": action.tool_input,
                    "tool_call_count": context.tool_call_count,
                })),
            );
        }
        Ok(None)
    }

    async fn after_tool_call(
        &self,
        action: &AgentAction,
        observation: &str,
        context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        if self.log_tool_calls {
            self.log(
                LogLevel::Info,
                "After tool call",
                Some(json!({
                    "tool": action.tool,
                    "observation_length": observation.len(),
                    "tool_call_count": context.tool_call_count,
                })),
            );
        }
        Ok(None)
    }

    async fn before_finish(
        &self,
        finish: &AgentFinish,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentFinish>, MiddlewareError> {
        if self.log_finish {
            self.log(
                LogLevel::Info,
                "Before finish",
                Some(json!({
                    "output": finish.output,
                    "total_iterations": context.iteration,
                    "total_tool_calls": context.tool_call_count,
                    "duration_ms": context.start_time.elapsed().as_millis(),
                })),
            );
        }
        Ok(None)
    }

    async fn after_finish(
        &self,
        finish: &AgentFinish,
        result: &GenerateResult,
        context: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        if self.log_finish {
            self.log(
                LogLevel::Info,
                "After finish",
                Some(json!({
                    "output": finish.output,
                    "result_generation": result.generation,
                    "total_iterations": context.iteration,
                    "total_tool_calls": context.tool_call_count,
                    "duration_ms": context.start_time.elapsed().as_millis(),
                })),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_middleware_creation() {
        let middleware = LoggingMiddleware::new();
        assert_eq!(middleware.log_level, LogLevel::Info);
        assert!(!middleware.structured);
    }

    #[test]
    fn test_log_level_filtering() {
        let middleware = LoggingMiddleware::new().with_log_level(LogLevel::Warn);
        assert!(!middleware.should_log(LogLevel::Debug));
        assert!(!middleware.should_log(LogLevel::Info));
        assert!(middleware.should_log(LogLevel::Warn));
        assert!(middleware.should_log(LogLevel::Error));
    }
}
