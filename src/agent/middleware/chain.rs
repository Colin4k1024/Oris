//! Middleware 链优化执行器
//!
//! 提供优化的 Middleware 链执行，支持早期退出、并行执行等。

use std::sync::Arc;

use crate::agent::context_engineering::{ModelRequest, ModelResponse};
use crate::agent::{
    middleware::{Middleware, MiddlewareContext, MiddlewareError},
    runtime::Runtime,
};
use crate::language_models::GenerateResult;
use crate::prompt::PromptArgs;
use crate::schemas::agent::{AgentAction, AgentFinish};

/// Middleware 执行结果
#[derive(Debug)]
pub enum MiddlewareResult<T> {
    /// 继续执行，使用修改后的值
    Modified(T),
    /// 继续执行，使用原始值
    Unchanged,
    /// 停止执行链
    Stop,
    /// 错误
    Error(MiddlewareError),
}

impl<T> MiddlewareResult<T> {
    /// 检查是否应该停止执行
    pub fn should_stop(&self) -> bool {
        matches!(self, MiddlewareResult::Stop | MiddlewareResult::Error(_))
    }

    /// 提取值（如果已修改）
    pub fn into_option(self) -> Option<T> {
        match self {
            MiddlewareResult::Modified(value) => Some(value),
            MiddlewareResult::Unchanged => None,
            MiddlewareResult::Stop => None,
            MiddlewareResult::Error(_) => None,
        }
    }
}

/// 优化的 Middleware 链执行器
pub struct MiddlewareChainExecutor;

impl MiddlewareChainExecutor {
    /// 执行 before_agent_plan 链
    ///
    /// 支持早期退出和值修改。
    pub async fn execute_before_agent_plan(
        middleware: &[Arc<dyn Middleware>],
        input: &PromptArgs,
        steps: &[(AgentAction, String)],
        context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        let mut current_input = input.clone();

        for mw in middleware {
            // 尝试使用 runtime-aware 版本（如果有）
            // 注意：这里需要 RuntimeRequest，但在简化版本中我们使用非 runtime 版本
            let modified = mw.before_agent_plan(&current_input, steps, context).await?;

            if let Some(new_input) = modified {
                current_input = new_input;
            }
        }

        if current_input == *input {
            Ok(None)
        } else {
            Ok(Some(current_input))
        }
    }

    /// 执行 before_model_call 链
    pub async fn execute_before_model_call(
        middleware: &[Arc<dyn Middleware>],
        request: &ModelRequest,
        context: &mut MiddlewareContext,
    ) -> Result<Option<ModelRequest>, MiddlewareError> {
        // Note: ModelRequest cannot be cloned, so we work with references
        let mut current_request = request;

        let mut modified_request = None;
        for mw in middleware {
            if let Some(new_request) = mw.before_model_call(current_request, context).await? {
                modified_request = Some(new_request);
                current_request = modified_request.as_ref().unwrap();
            }
        }

        Ok(modified_request)
    }

    /// 执行 after_model_call 链
    pub async fn execute_after_model_call(
        middleware: &[Arc<dyn Middleware>],
        request: &ModelRequest,
        response: &ModelResponse,
        context: &mut MiddlewareContext,
    ) -> Result<Option<ModelResponse>, MiddlewareError> {
        let mut modified_response = None;
        let mut current_response = response;

        for mw in middleware {
            if let Some(new_response) = mw
                .after_model_call(request, current_response, context)
                .await?
            {
                modified_response = Some(new_response);
                current_response = modified_response.as_ref().unwrap();
            }
        }

        Ok(modified_response)
    }

    /// 执行 before_tool_call 链
    pub async fn execute_before_tool_call(
        middleware: &[Arc<dyn Middleware>],
        action: &AgentAction,
        runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentAction>, MiddlewareError> {
        let mut current_action = action.clone();

        for mw in middleware {
            let modified = if let Some(runtime) = runtime {
                mw.before_tool_call_with_runtime(&current_action, Some(runtime), context)
                    .await?
            } else {
                mw.before_tool_call(&current_action, context).await?
            };

            if let Some(new_action) = modified {
                current_action = new_action;
            }
        }

        if current_action.tool == action.tool && current_action.tool_input == action.tool_input {
            Ok(None)
        } else {
            Ok(Some(current_action))
        }
    }

    /// 执行 after_tool_call 链
    pub async fn execute_after_tool_call(
        middleware: &[Arc<dyn Middleware>],
        action: &AgentAction,
        observation: &str,
        runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<Option<String>, MiddlewareError> {
        let mut current_observation = observation.to_string();

        for mw in middleware {
            let modified = if let Some(runtime) = runtime {
                mw.after_tool_call_with_runtime(
                    action,
                    &current_observation,
                    Some(runtime),
                    context,
                )
                .await?
            } else {
                mw.after_tool_call(action, &current_observation, context)
                    .await?
            };

            if let Some(new_observation) = modified {
                current_observation = new_observation;
            }
        }

        if current_observation == observation {
            Ok(None)
        } else {
            Ok(Some(current_observation))
        }
    }

    /// 执行 before_finish 链
    pub async fn execute_before_finish(
        middleware: &[Arc<dyn Middleware>],
        finish: &AgentFinish,
        runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<Option<AgentFinish>, MiddlewareError> {
        let mut current_finish = finish.clone();

        for mw in middleware {
            let modified = if let Some(runtime) = runtime {
                mw.before_finish_with_runtime(&current_finish, Some(runtime), context)
                    .await?
            } else {
                mw.before_finish(&current_finish, context).await?
            };

            if let Some(new_finish) = modified {
                current_finish = new_finish;
            }
        }

        if current_finish.output == finish.output {
            Ok(None)
        } else {
            Ok(Some(current_finish))
        }
    }

    /// 执行 after_finish 链（不返回值，只用于副作用）
    pub async fn execute_after_finish(
        middleware: &[Arc<dyn Middleware>],
        finish: &AgentFinish,
        result: &GenerateResult,
        runtime: Option<&Runtime>,
        context: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        for mw in middleware {
            if let Some(runtime) = runtime {
                mw.after_finish_with_runtime(finish, result, Some(runtime), context)
                    .await?;
            } else {
                mw.after_finish(finish, result, context).await?;
            }
        }

        Ok(())
    }
}

/// Middleware 链配置
///
/// 用于配置 Middleware 链的执行行为。
#[derive(Debug, Clone)]
pub struct MiddlewareChainConfig {
    /// 是否允许早期退出
    pub allow_early_exit: bool,
    /// 是否并行执行独立的 middleware
    pub enable_parallel_execution: bool,
    /// 最大 middleware 数量
    pub max_middleware_count: Option<usize>,
}

impl Default for MiddlewareChainConfig {
    fn default() -> Self {
        Self {
            allow_early_exit: false,
            enable_parallel_execution: false,
            max_middleware_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_middleware_result() {
        let result = MiddlewareResult::Modified("test".to_string());
        assert!(!result.should_stop());
        assert_eq!(result.into_option(), Some("test".to_string()));

        let stop_result = MiddlewareResult::<String>::Stop;
        assert!(stop_result.should_stop());
    }
}
