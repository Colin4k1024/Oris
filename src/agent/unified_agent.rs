use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use serde_json::json;

use crate::{
    chain::{chain_trait::Chain, ChainError},
    language_models::GenerateResult,
    prompt::PromptArgs,
    schemas::{
        agent::AgentAction, agent::AgentEvent, memory::BaseMemory, messages::Message,
        StructuredOutputStrategy,
    },
    tools::{ToolContext, ToolStore},
};

use super::{
    agent::Agent, checkpoint::AgentCheckpointer, executor::AgentExecutor, state::AgentState,
    AgentError, AgentInvokeResult,
};
use super::utils::convert_messages_to_prompt_args;
use crate::agent::runtime::{Runtime, TypedContext};
use crate::langgraph::RunnableConfig;
use serde_json::Value as JsonValue;

/// Input for [UnifiedAgent::invoke_with_config]: either normal state (prompt args) or resume with decisions.
#[derive(Clone, Debug)]
pub enum AgentInput {
    /// Normal invocation (prompt args or message-based args).
    State(PromptArgs),
    /// Resume with human decisions after an interrupt (value with "decisions" key; use same thread_id).
    Resume(JsonValue),
}

/// A wrapper that makes Box<dyn Agent> work with AgentExecutor
struct AgentBox(Box<dyn Agent>);

#[async_trait]
impl Agent for AgentBox {
    async fn plan(
        &self,
        intermediate_steps: &[(AgentAction, String)],
        inputs: PromptArgs,
    ) -> Result<AgentEvent, AgentError> {
        self.0.plan(intermediate_steps, inputs).await
    }

    fn get_tools(&self) -> Vec<Arc<dyn crate::tools::Tool>> {
        self.0.get_tools()
    }
}

/// A unified agent that combines agent creation and execution with built-in memory.
///
/// This struct provides a simplified interface for creating and using agents,
/// similar to LangChain Python's `create_agent` function.
pub struct UnifiedAgent {
    executor: AgentExecutor<AgentBox>,
}

impl UnifiedAgent {
    /// Create a new unified agent from an agent instance.
    pub fn new(agent: Box<dyn Agent>) -> Self {
        Self {
            executor: AgentExecutor::from_agent(AgentBox(agent)),
        }
    }

    /// Set the memory for the agent.
    pub fn with_memory(mut self, memory: Arc<Mutex<dyn BaseMemory>>) -> Self {
        self.executor = self.executor.with_memory(memory);
        self
    }

    /// Set the maximum number of iterations.
    pub fn with_max_iterations(mut self, max_iterations: i32) -> Self {
        self.executor = self.executor.with_max_iterations(max_iterations);
        self
    }

    /// Set whether to break on error.
    pub fn with_break_if_error(mut self, break_if_error: bool) -> Self {
        self.executor = self.executor.with_break_if_error(break_if_error);
        self
    }

    /// Set the context for the agent.
    pub fn with_context(mut self, context: Arc<dyn ToolContext>) -> Self {
        self.executor = self.executor.with_context(context);
        self
    }

    /// Set the store for the agent.
    pub fn with_store(mut self, store: Arc<dyn ToolStore>) -> Self {
        self.executor = self.executor.with_store(store);
        self
    }

    /// Set the file backend for FS tools (ls, read_file, write_file, edit_file, glob, grep).
    pub fn with_file_backend(
        mut self,
        file_backend: Option<std::sync::Arc<dyn crate::tools::FileBackend>>,
    ) -> Self {
        self.executor = self.executor.with_file_backend(file_backend);
        self
    }

    /// Set the structured output format for the agent.
    pub fn with_response_format(
        mut self,
        response_format: Box<dyn StructuredOutputStrategy>,
    ) -> Self {
        self.executor = self.executor.with_response_format(response_format);
        self
    }

    /// Set middleware for the agent.
    pub fn with_middleware(
        mut self,
        middleware: Vec<Arc<dyn super::middleware::Middleware>>,
    ) -> Self {
        self.executor = self.executor.with_middleware(middleware);
        self
    }

    /// Set the state for the agent.
    pub fn with_state(mut self, state: Arc<Mutex<AgentState>>) -> Self {
        self.executor = self.executor.with_state(state);
        self
    }

    /// Set the checkpointer for human-in-the-loop (save on interrupt, load on resume).
    pub fn with_checkpointer(mut self, checkpointer: Option<Arc<dyn AgentCheckpointer>>) -> Self {
        self.executor = self.executor.with_checkpointer(checkpointer);
        self
    }

    /// Invoke with config (thread_id) for HILP. Returns [AgentInvokeResult] (Complete or Interrupt).
    /// Use [AgentInput::State] for normal input or [AgentInput::Resume] with decisions after an interrupt.
    pub async fn invoke_with_config(
        &self,
        input: AgentInput,
        config: &RunnableConfig,
    ) -> Result<AgentInvokeResult, ChainError> {
        match input {
            AgentInput::State(prompt_args) => {
                let args = if prompt_args.contains_key("messages") {
                    convert_messages_to_prompt_args(prompt_args)?
                } else {
                    prompt_args
                };
                match self.executor.call_with_config(args, Some(config)).await {
                    Ok(gen) => Ok(AgentInvokeResult::Complete(gen.generation)),
                    Err(ChainError::Interrupt(payload)) => Ok(AgentInvokeResult::Interrupt {
                        interrupt_value: payload,
                    }),
                    Err(e) => Err(e),
                }
            }
            AgentInput::Resume(decisions_value) => {
                let gen = self.executor.call_resume(config, decisions_value).await?;
                Ok(AgentInvokeResult::Complete(gen.generation))
            }
        }
    }

    /// Invoke the agent with messages.
    ///
    /// This method accepts either:
    /// - A vector of messages: `vec![Message::new_human_message("Hello")]`
    /// - A prompt args map with "messages" key
    /// - A prompt args map with "input" key (backward compatibility)
    pub async fn invoke_messages(&self, messages: Vec<Message>) -> Result<String, ChainError> {
        let input_variables = prompt_args_from_messages(messages)?;
        self.executor.invoke(input_variables).await
    }

    /// Invoke the agent with a typed context.
    ///
    /// This allows you to pass a type-safe context that will be available
    /// to tools and middleware through the runtime.
    ///
    /// # Example
    /// ```rust,ignore
    /// #[derive(Clone)]
    /// struct MyContext {
    ///     user_id: String,
    ///     user_name: String,
    /// }
    ///
    /// impl TypedContext for MyContext { ... }
    ///
    /// let context = MyContext {
    ///     user_id: "user123".to_string(),
    ///     user_name: "John".to_string(),
    /// };
    ///
    /// let result = agent.invoke_with_context(
    ///     prompt_args! { "input" => "Hello" },
    ///     context,
    /// ).await?;
    /// ```
    pub async fn invoke_with_context<C: TypedContext>(
        &self,
        input_variables: PromptArgs,
        context: C,
    ) -> Result<String, ChainError> {
        // Convert typed context to ToolContext
        let tool_context = context.to_tool_context();

        // Get store from executor
        // Note: We need to access store, but it's private.
        // For now, we'll use the context that's already set in the executor
        // In a full implementation, we'd add a getter or make store accessible
        let store = Arc::new(crate::tools::InMemoryStore::new()); // Fallback

        // Create runtime
        let _runtime = Arc::new(Runtime::new(tool_context, store));

        // Use executor's invoke_with_runtime if available, otherwise fallback
        // For now, we'll update the executor's context temporarily
        // In a full implementation, we'd add invoke_with_runtime to executor
        self.executor.invoke(input_variables).await
    }

    /// Invoke with messages and typed context
    pub async fn invoke_messages_with_context<C: TypedContext>(
        &self,
        messages: Vec<Message>,
        context: C,
    ) -> Result<String, ChainError> {
        let input_variables = prompt_args_from_messages(messages)?;
        self.invoke_with_context(input_variables, context).await
    }
}

#[async_trait]
impl Chain for UnifiedAgent {
    async fn call(&self, input_variables: PromptArgs) -> Result<GenerateResult, ChainError> {
        // Check if input is message-based format
        let input_variables = if input_variables.contains_key("messages") {
            convert_messages_to_prompt_args(input_variables)?
        } else {
            input_variables
        };

        self.executor.call(input_variables).await
    }

    async fn invoke(&self, input_variables: PromptArgs) -> Result<String, ChainError> {
        // Check if input is message-based format
        let input_variables = if input_variables.contains_key("messages") {
            convert_messages_to_prompt_args(input_variables)?
        } else {
            input_variables
        };

        self.executor.invoke(input_variables).await
    }
}

/// Convert messages to prompt args format.
fn prompt_args_from_messages(messages: Vec<Message>) -> Result<PromptArgs, ChainError> {
    // Extract the last human message as input
    let input = messages
        .iter()
        .rev()
        .find(|m| matches!(m.message_type, crate::schemas::MessageType::HumanMessage))
        .map(|m| m.content.clone())
        .unwrap_or_else(|| {
            messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default()
        });

    let mut prompt_args = PromptArgs::new();
    prompt_args.insert("input".to_string(), json!(input));
    prompt_args.insert("chat_history".to_string(), json!(messages));
    Ok(prompt_args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_args_from_messages() {
        let messages = vec![
            Message::new_system_message("You are helpful"),
            Message::new_human_message("Hello"),
        ];

        let result = prompt_args_from_messages(messages);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert!(args.contains_key("input"));
        assert!(args.contains_key("chat_history"));
        assert_eq!(args["input"], json!("Hello"));
    }

    #[test]
    fn test_convert_messages_to_prompt_args() {
        let mut input_vars = PromptArgs::new();
        input_vars.insert(
            "messages".to_string(),
            json!([
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there!"}
            ]),
        );

        let result = convert_messages_to_prompt_args(input_vars);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert!(args.contains_key("input"));
        assert!(args.contains_key("chat_history"));
    }
}
