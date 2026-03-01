use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::Mutex;

use super::middleware::human_in_loop::{
    CURRENT_BATCH_ACTIONS_KEY, RESUME_DECISIONS_KEY, RESUME_DECISION_INDEX_KEY,
};
use super::{
    agent::Agent,
    checkpoint::{AgentCheckpointState, AgentCheckpointer},
    context_engineering::{ModelRequest, ModelResponse},
    message_repair,
    middleware::Middleware,
    middleware::MiddlewareContext,
    middleware::MiddlewareError,
    runtime::{Runtime, RuntimeRequest},
    state::AgentState,
    AgentError,
};
use crate::schemas::{LogTools, Message};
use crate::{
    chain::{chain_trait::Chain, ChainError},
    language_models::GenerateResult,
    memory::SimpleMemory,
    prompt::PromptArgs,
    schemas::{
        agent::{AgentAction, AgentEvent},
        memory::BaseMemory,
        StructuredOutputStrategy,
    },
    tools::{FileBackend, Tool, ToolContext, ToolRuntime, ToolStore},
};

// Re-export the shared utility function
pub use super::utils::convert_messages_to_prompt_args;

pub struct AgentExecutor<A>
where
    A: Agent,
{
    agent: A,
    max_iterations: Option<i32>,
    break_if_error: bool,
    pub memory: Option<Arc<Mutex<dyn BaseMemory>>>,
    state: Arc<Mutex<AgentState>>,
    context: Arc<dyn ToolContext>,
    pub(crate) store: Arc<dyn ToolStore>,
    response_format: Option<Box<dyn StructuredOutputStrategy>>,
    middleware: Vec<Arc<dyn Middleware>>,
    file_backend: Option<Arc<dyn FileBackend>>,
    /// Checkpointer for human-in-the-loop: save state on interrupt, load on resume.
    checkpointer: Option<Arc<dyn AgentCheckpointer>>,
}

impl<A> AgentExecutor<A>
where
    A: Agent,
{
    pub fn from_agent(agent: A) -> Self {
        Self {
            agent,
            max_iterations: Some(10),
            break_if_error: false,
            memory: None,
            state: Arc::new(Mutex::new(AgentState::new())),
            context: Arc::new(crate::tools::EmptyContext),
            store: Arc::new(crate::tools::InMemoryStore::new()),
            response_format: None,
            middleware: Vec::new(),
            file_backend: None,
            checkpointer: None,
        }
    }

    /// Set checkpointer for human-in-the-loop (save on interrupt, load on resume).
    pub fn with_checkpointer(mut self, checkpointer: Option<Arc<dyn AgentCheckpointer>>) -> Self {
        self.checkpointer = checkpointer;
        self
    }

    pub fn with_file_backend(mut self, file_backend: Option<Arc<dyn FileBackend>>) -> Self {
        self.file_backend = file_backend;
        self
    }

    pub fn with_context(mut self, context: Arc<dyn ToolContext>) -> Self {
        self.context = context;
        self
    }

    pub fn with_store(mut self, store: Arc<dyn ToolStore>) -> Self {
        self.store = store;
        self
    }

    pub fn with_state(mut self, state: Arc<Mutex<AgentState>>) -> Self {
        self.state = state;
        self
    }

    pub fn with_max_iterations(mut self, max_iterations: i32) -> Self {
        self.max_iterations = Some(max_iterations);
        self
    }

    pub fn with_memory(mut self, memory: Arc<Mutex<dyn BaseMemory>>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_break_if_error(mut self, break_if_error: bool) -> Self {
        self.break_if_error = break_if_error;
        self
    }

    pub fn with_response_format(
        mut self,
        response_format: Box<dyn StructuredOutputStrategy>,
    ) -> Self {
        self.response_format = Some(response_format);
        self
    }

    pub fn with_middleware(mut self, middleware: Vec<Arc<dyn Middleware>>) -> Self {
        self.middleware = middleware;
        self
    }

    fn get_name_to_tools(&self) -> HashMap<String, Arc<dyn Tool>> {
        let mut name_to_tool = HashMap::new();
        for tool in self.agent.get_tools().iter() {
            log::debug!("Loading Tool:{}", tool.name());
            name_to_tool.insert(tool.name().trim().replace(" ", "_"), tool.clone());
        }
        name_to_tool
    }

    async fn handle_command(
        &self,
        command: crate::agent::state::Command,
    ) -> Result<(), ChainError> {
        let mut state = self.state.lock().await;
        match command {
            crate::agent::state::Command::UpdateState { fields } => {
                for (key, value) in fields {
                    state.set_field(key, value);
                }
            }
            crate::agent::state::Command::RemoveMessages { ids } => {
                state.messages.retain(|msg| {
                    !ids.contains(
                        &msg.id
                            .as_ref()
                            .map(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                    )
                });
            }
            crate::agent::state::Command::ClearMessages => {
                state.messages.clear();
            }
            crate::agent::state::Command::ClearState => {
                state.messages.clear();
                state.custom_fields.clear();
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<A> Chain for AgentExecutor<A>
where
    A: Agent + Send + Sync,
{
    async fn call(&self, input_variables: PromptArgs) -> Result<GenerateResult, ChainError> {
        self.run_loop(input_variables, None, None).await
    }

    async fn invoke(&self, input_variables: PromptArgs) -> Result<String, ChainError> {
        let result = self.call(input_variables).await?;
        Ok(result.generation)
    }
}

impl<A> AgentExecutor<A>
where
    A: Agent + Send + Sync,
{
    /// Run the agent loop with optional config (for HILP checkpoint) and optional resume state.
    pub async fn run_loop(
        &self,
        input_variables: PromptArgs,
        config: Option<&crate::graph::RunnableConfig>,
        resume: Option<(AgentCheckpointState, serde_json::Value)>,
    ) -> Result<GenerateResult, ChainError> {
        let input_variables = if input_variables.contains_key("messages") {
            convert_messages_to_prompt_args(input_variables)?
        } else {
            input_variables.clone()
        };
        let name_to_tools = self.get_name_to_tools();
        let mut steps: Vec<(AgentAction, String)> = Vec::new();
        let mut plan_input = input_variables.clone();
        let mut resume_batch: Option<(Vec<AgentAction>, serde_json::Value)> = None;
        let had_resume = resume.is_some();
        if let Some((state, ref decisions)) = resume {
            steps = state.steps;
            plan_input = state.input_variables;
            resume_batch = Some((state.pending_actions, decisions.clone()));
        }
        let mut first_normal_after_resume = had_resume;
        let mut middleware_context = MiddlewareContext::new();
        if resume_batch.is_none() {
            middleware_context
                .set_custom_data(RESUME_DECISION_INDEX_KEY.to_string(), serde_json::json!(0));
        }
        if let Some((_, ref decisions)) = resume_batch {
            if let Some(decisions_arr) = decisions.get("decisions") {
                middleware_context
                    .set_custom_data(RESUME_DECISIONS_KEY.to_string(), decisions_arr.clone());
                middleware_context
                    .set_custom_data(RESUME_DECISION_INDEX_KEY.to_string(), serde_json::json!(0));
            }
        }
        log::debug!("steps: {:?}", steps);
        if !had_resume {
            if let Some(memory) = &self.memory {
                let memory = memory.lock().await;
                let mut history = memory.messages();
                history = message_repair::repair_dangling_tool_calls(history);
                plan_input.insert("chat_history".to_string(), json!(history));
            } else {
                let mut history = SimpleMemory::new().messages();
                history = message_repair::repair_dangling_tool_calls(history);
                plan_input.insert("chat_history".to_string(), json!(history));
            }
        }

        let runtime = Arc::new(Runtime::new(
            Arc::clone(&self.context),
            Arc::clone(&self.store),
        ));

        loop {
            // Process resumed batch (pending actions with decisions)
            if let Some((pending_actions, _)) = resume_batch.take() {
                let pending_for_checkpoint = pending_actions.clone();
                middleware_context.set_custom_data(
                    CURRENT_BATCH_ACTIONS_KEY.to_string(),
                    serde_json::to_value(&pending_actions).unwrap_or_default(),
                );
                for mut action in pending_actions {
                    let mut reject_this_tool = false;
                    for mw in &self.middleware {
                        let res = mw
                            .before_tool_call_with_runtime(
                                &action,
                                Some(&*runtime),
                                &mut middleware_context,
                            )
                            .await;
                        match &res {
                            Err(MiddlewareError::Interrupt(p)) => {
                                if let (Some(cp), Some(tid)) = (
                                    self.checkpointer.as_ref(),
                                    config.and_then(|c| c.get_thread_id()),
                                ) {
                                    cp.put_async(
                                        &tid,
                                        &AgentCheckpointState {
                                            steps: steps.clone(),
                                            input_variables: plan_input.clone(),
                                            pending_actions: pending_for_checkpoint.clone(),
                                        },
                                    )
                                    .await;
                                }
                                return Err(ChainError::Interrupt(p.clone()));
                            }
                            Err(MiddlewareError::RejectTool) => {
                                reject_this_tool = true;
                                break;
                            }
                            Err(e) => return Err(ChainError::AgentError(e.to_string())),
                            Ok(Some(m)) => action = m.clone(),
                            Ok(None) => {}
                        }
                        if matches!(res, Ok(Some(_))) {
                            continue;
                        }
                        {
                            let res2 = mw.before_tool_call(&action, &mut middleware_context).await;
                            match res2 {
                                Err(MiddlewareError::Interrupt(p)) => {
                                    if let (Some(cp), Some(tid)) = (
                                        self.checkpointer.as_ref(),
                                        config.and_then(|c| c.get_thread_id()),
                                    ) {
                                        cp.put_async(
                                            &tid,
                                            &AgentCheckpointState {
                                                steps: steps.clone(),
                                                input_variables: plan_input.clone(),
                                                pending_actions: pending_for_checkpoint.clone(),
                                            },
                                        )
                                        .await;
                                    }
                                    return Err(ChainError::Interrupt(p));
                                }
                                Err(MiddlewareError::RejectTool) => {
                                    reject_this_tool = true;
                                    break;
                                }
                                Err(e) => return Err(ChainError::AgentError(e.to_string())),
                                Ok(Some(m)) => action = m,
                                Ok(None) => {}
                            }
                        }
                    }
                    if reject_this_tool {
                        steps.push((action, "Tool call rejected by user.".to_string()));
                        continue;
                    }
                    middleware_context.increment_tool_call_count();
                    let tool = name_to_tools
                        .get(&action.tool.trim().replace(" ", "_"))
                        .ok_or_else(|| {
                            AgentError::ToolError(format!("Tool {} not found", action.tool))
                        })
                        .map_err(|e| ChainError::AgentError(e.to_string()))?;
                    let tool_call_id = format!("call_{}", steps.len());
                    let mut tool_runtime = ToolRuntime::new(
                        Arc::clone(&self.state),
                        Arc::clone(&self.context),
                        Arc::clone(&self.store),
                        tool_call_id,
                    );
                    if let Some(ref fb) = self.file_backend {
                        tool_runtime = tool_runtime.with_file_backend(Arc::clone(fb));
                    }
                    let observation_result: Result<String, String> = if tool.requires_runtime() {
                        let input = tool.parse_input(&action.tool_input).await;
                        tool.run_with_runtime(input, &tool_runtime)
                            .await
                            .map(|result| result.into_string())
                            .map_err(|e| e.to_string())
                    } else {
                        tool.call(&action.tool_input)
                            .await
                            .map_err(|e| e.to_string())
                    };
                    let mut observation = match observation_result {
                        Ok(result) => result,
                        Err(error_msg) => {
                            if self.break_if_error {
                                return Err(ChainError::AgentError(
                                    AgentError::ToolError(error_msg).to_string(),
                                ));
                            }
                            format!("The tool return the following error: {}", error_msg)
                        }
                    };
                    for mw in &self.middleware {
                        let modified = mw
                            .after_tool_call_with_runtime(
                                &action,
                                &observation,
                                Some(&*runtime),
                                &mut middleware_context,
                            )
                            .await
                            .map_err(|e| {
                                ChainError::AgentError(format!("Middleware error: {}", e))
                            })?;
                        if let Some(mo) = modified {
                            observation = mo;
                        } else if let Some(mo) = mw
                            .after_tool_call(&action, &observation, &mut middleware_context)
                            .await
                            .map_err(|e| {
                                ChainError::AgentError(format!("Middleware error: {}", e))
                            })?
                        {
                            observation = mo;
                        }
                    }
                    steps.push((action, observation));
                }
                continue;
            }

            if !first_normal_after_resume {
                plan_input = input_variables.clone();
            }
            first_normal_after_resume = false;

            middleware_context.increment_iteration();

            // Create runtime request
            let runtime_request = RuntimeRequest::new(plan_input.clone(), Arc::clone(&self.state))
                .with_runtime(Arc::clone(&runtime));

            // Apply before_agent_plan hooks (try runtime-aware version first)
            for mw in &self.middleware {
                // Try runtime-aware hook first
                let modified = mw
                    .before_agent_plan_with_runtime(
                        &runtime_request,
                        &steps,
                        &mut middleware_context,
                    )
                    .await
                    .map_err(|e| ChainError::AgentError(format!("Middleware error: {}", e)))?;

                if let Some(modified_input) = modified {
                    plan_input = modified_input;
                } else {
                    // Fallback to non-runtime hook
                    if let Some(modified_input) = mw
                        .before_agent_plan(&plan_input, &steps, &mut middleware_context)
                        .await
                        .map_err(|e| ChainError::AgentError(format!("Middleware error: {}", e)))?
                    {
                        plan_input = modified_input;
                    }
                }
            }

            // Create ModelRequest for context engineering
            // Extract messages from plan_input (if available)
            let mut messages = Vec::new();
            if let Some(chat_history) = plan_input.get("chat_history") {
                if let Ok(msgs) = serde_json::from_value::<Vec<Message>>(chat_history.clone()) {
                    messages = msgs;
                }
            }

            // Get tools from agent
            let tools = self.agent.get_tools();

            // Create ModelRequest
            let mut model_request = ModelRequest::new(messages, tools, Arc::clone(&self.state))
                .with_runtime(Arc::clone(&runtime));

            // Apply before_model_call hooks
            for mw in &self.middleware {
                if let Some(modified_request) = mw
                    .before_model_call(&model_request, &mut middleware_context)
                    .await
                    .map_err(|e| ChainError::AgentError(format!("Middleware error: {}", e)))?
                {
                    model_request = modified_request;

                    // Update plan_input with modified messages
                    if !model_request.messages.is_empty() {
                        plan_input.insert(
                            "chat_history".to_string(),
                            serde_json::json!(model_request.messages),
                        );
                    }

                    // Note: Tool filtering would need to be handled at agent level
                    // For now, we'll pass the filtered tools through metadata
                    // In practice, you'd need to modify the agent interface or use a wrapper
                }
            }

            let mut agent_event = self
                .agent
                .plan(&steps, plan_input.clone())
                .await
                .map_err(|e| ChainError::AgentError(format!("Error in agent planning: {}", e)))?;

            // Create ModelResponse (simplified - actual response comes from agent)
            let model_response = ModelResponse::new(GenerateResult {
                generation: String::new(),
                ..Default::default()
            });

            // Apply after_model_call hooks
            for mw in &self.middleware {
                if let Some(_modified_response) = mw
                    .after_model_call(&model_request, &model_response, &mut middleware_context)
                    .await
                    .map_err(|e| ChainError::AgentError(format!("Middleware error: {}", e)))?
                {
                    // Response modifications would be applied here
                }
            }

            // Apply after_agent_plan hooks (try runtime-aware version first)
            let runtime_request = RuntimeRequest::new(plan_input.clone(), Arc::clone(&self.state))
                .with_runtime(Arc::clone(&runtime));

            for mw in &self.middleware {
                // Try runtime-aware hook first
                let modified = mw
                    .after_agent_plan_with_runtime(
                        &runtime_request,
                        &agent_event,
                        &mut middleware_context,
                    )
                    .await
                    .map_err(|e| ChainError::AgentError(format!("Middleware error: {}", e)))?;

                if let Some(modified_event) = modified {
                    agent_event = modified_event;
                } else {
                    // Fallback to non-runtime hook
                    if let Some(modified_event) = mw
                        .after_agent_plan(&plan_input, &agent_event, &mut middleware_context)
                        .await
                        .map_err(|e| ChainError::AgentError(format!("Middleware error: {}", e)))?
                    {
                        agent_event = modified_event;
                    }
                }
            }
            match agent_event {
                AgentEvent::Action(actions) => {
                    let actions_for_checkpoint = actions.clone();
                    middleware_context.set_custom_data(
                        CURRENT_BATCH_ACTIONS_KEY.to_string(),
                        serde_json::to_value(&actions).unwrap_or_default(),
                    );
                    for mut action in actions {
                        let mut reject_this_tool = false;
                        for mw in &self.middleware {
                            let res = mw
                                .before_tool_call_with_runtime(
                                    &action,
                                    Some(&*runtime),
                                    &mut middleware_context,
                                )
                                .await;
                            match &res {
                                Err(MiddlewareError::Interrupt(p)) => {
                                    if let (Some(cp), Some(tid)) = (
                                        self.checkpointer.as_ref(),
                                        config.and_then(|c| c.get_thread_id()),
                                    ) {
                                        cp.put_async(
                                            &tid,
                                            &AgentCheckpointState {
                                                steps: steps.clone(),
                                                input_variables: plan_input.clone(),
                                                pending_actions: actions_for_checkpoint.clone(),
                                            },
                                        )
                                        .await;
                                    }
                                    return Err(ChainError::Interrupt(p.clone()));
                                }
                                Err(MiddlewareError::RejectTool) => {
                                    reject_this_tool = true;
                                    break;
                                }
                                Err(e) => return Err(ChainError::AgentError(e.to_string())),
                                Ok(Some(m)) => action = m.clone(),
                                Ok(None) => {}
                            }
                            if matches!(res, Ok(Some(_))) {
                                continue;
                            }
                            let res2 = mw.before_tool_call(&action, &mut middleware_context).await;
                            match res2 {
                                Err(MiddlewareError::Interrupt(p)) => {
                                    if let (Some(cp), Some(tid)) = (
                                        self.checkpointer.as_ref(),
                                        config.and_then(|c| c.get_thread_id()),
                                    ) {
                                        cp.put_async(
                                            &tid,
                                            &AgentCheckpointState {
                                                steps: steps.clone(),
                                                input_variables: plan_input.clone(),
                                                pending_actions: actions_for_checkpoint.clone(),
                                            },
                                        )
                                        .await;
                                    }
                                    return Err(ChainError::Interrupt(p));
                                }
                                Err(MiddlewareError::RejectTool) => {
                                    reject_this_tool = true;
                                    break;
                                }
                                Err(e) => return Err(ChainError::AgentError(e.to_string())),
                                Ok(Some(m)) => action = m,
                                Ok(None) => {}
                            }
                        }
                        if reject_this_tool {
                            steps.push((action, "Tool call rejected by user.".to_string()));
                            continue;
                        }

                        log::debug!("Action: {:?}", action.tool_input);
                        middleware_context.increment_tool_call_count();

                        let tool = name_to_tools
                            .get(&action.tool.trim().replace(" ", "_"))
                            .ok_or_else(|| {
                                AgentError::ToolError(format!("Tool {} not found", action.tool))
                            })
                            .map_err(|e| ChainError::AgentError(e.to_string()))?;

                        // Create ToolRuntime for tools that need it
                        let tool_call_id = format!("call_{}", steps.len());
                        let mut tool_runtime = ToolRuntime::new(
                            Arc::clone(&self.state),
                            Arc::clone(&self.context),
                            Arc::clone(&self.store),
                            tool_call_id,
                        );
                        if let Some(ref fb) = self.file_backend {
                            tool_runtime = tool_runtime.with_file_backend(Arc::clone(fb));
                        }

                        // Check if tool requires runtime
                        let observation_result: Result<String, String> = if tool.requires_runtime()
                        {
                            let input = tool.parse_input(&action.tool_input).await;
                            tool.run_with_runtime(input, &tool_runtime)
                                .await
                                .map(|result| result.into_string())
                                .map_err(|e| e.to_string())
                        } else {
                            tool.call(&action.tool_input)
                                .await
                                .map_err(|e| e.to_string())
                        };

                        let mut observation = match observation_result {
                            Ok(result) => result,
                            Err(error_msg) => {
                                log::info!("The tool return the following error: {}", error_msg);
                                if self.break_if_error {
                                    return Err(ChainError::AgentError(
                                        AgentError::ToolError(error_msg.clone()).to_string(),
                                    ));
                                } else {
                                    format!("The tool return the following error: {}", error_msg)
                                }
                            }
                        };

                        // Apply after_tool_call hooks (try runtime-aware version first)
                        for mw in &self.middleware {
                            // Try runtime-aware hook first
                            let modified = mw
                                .after_tool_call_with_runtime(
                                    &action,
                                    &observation,
                                    Some(&*runtime),
                                    &mut middleware_context,
                                )
                                .await
                                .map_err(|e| {
                                    ChainError::AgentError(format!("Middleware error: {}", e))
                                })?;

                            if let Some(modified_observation) = modified {
                                observation = modified_observation;
                            } else {
                                // Fallback to non-runtime hook
                                if let Some(modified_observation) = mw
                                    .after_tool_call(&action, &observation, &mut middleware_context)
                                    .await
                                    .map_err(|e| {
                                        ChainError::AgentError(format!("Middleware error: {}", e))
                                    })?
                                {
                                    observation = modified_observation;
                                }
                            }
                        }

                        steps.push((action, observation));
                    }
                }
                AgentEvent::Finish(mut finish) => {
                    // Apply before_finish hooks (try runtime-aware version first)
                    for mw in &self.middleware {
                        // Try runtime-aware hook first
                        let modified = mw
                            .before_finish_with_runtime(
                                &finish,
                                Some(&*runtime),
                                &mut middleware_context,
                            )
                            .await
                            .map_err(|e| {
                                ChainError::AgentError(format!("Middleware error: {}", e))
                            })?;

                        if let Some(modified_finish) = modified {
                            finish = modified_finish;
                        } else {
                            // Fallback to non-runtime hook
                            if let Some(modified_finish) = mw
                                .before_finish(&finish, &mut middleware_context)
                                .await
                                .map_err(|e| {
                                    ChainError::AgentError(format!("Middleware error: {}", e))
                                })?
                            {
                                finish = modified_finish;
                            }
                        }
                    }

                    if let Some(memory) = &self.memory {
                        let mut memory = memory.lock().await;

                        memory.add_user_message(match &input_variables["input"] {
                            // This avoids adding extra quotes to the user input in the history.
                            serde_json::Value::String(s) => s,
                            x => x, // this the json encoded value.
                        });

                        let mut tools_ai_message_seen: HashMap<String, ()> = HashMap::default();
                        for (action, observation) in steps {
                            match serde_json::from_str::<LogTools>(&action.log) {
                                Ok(LogTools { tool_id, tools }) => {
                                    let tools_value: serde_json::Value =
                                        serde_json::from_str(&tools).map_err(ChainError::from)?;
                                    if tools_ai_message_seen.insert(tools.clone(), ()).is_none() {
                                        memory.add_message(
                                            Message::new_ai_message("")
                                                .with_tool_calls(tools_value),
                                        );
                                    }
                                    memory.add_message(Message::new_tool_message(
                                        observation,
                                        tool_id,
                                    ));
                                }
                                Err(_) => {
                                    // Conversational agent: action.log is raw model text, not LogTools
                                    memory.add_message(Message::new_ai_message(&action.log));
                                    memory.add_message(Message::new_tool_message(
                                        observation,
                                        &action.tool,
                                    ));
                                }
                            }
                        }

                        memory.add_ai_message(&finish.output);
                    }

                    let result = GenerateResult {
                        generation: finish.output.clone(),
                        ..Default::default()
                    };

                    // Apply after_finish hooks (try runtime-aware version first)
                    for mw in &self.middleware {
                        // Try runtime-aware hook first
                        mw.after_finish_with_runtime(
                            &finish,
                            &result,
                            Some(&*runtime),
                            &mut middleware_context,
                        )
                        .await
                        .map_err(|e| ChainError::AgentError(format!("Middleware error: {}", e)))?;
                    }

                    return Ok(result);
                }
            }

            if let Some(max_iterations) = self.max_iterations {
                if steps.len() >= max_iterations as usize {
                    return Ok(GenerateResult {
                        generation: "Max iterations reached".to_string(),
                        ..Default::default()
                    });
                }
            }
        }
    }

    /// Run the agent with optional config (thread_id for HILP checkpointer). Returns interrupt payload on HILP interrupt.
    pub async fn call_with_config(
        &self,
        input_variables: PromptArgs,
        config: Option<&crate::graph::RunnableConfig>,
    ) -> Result<GenerateResult, ChainError> {
        self.run_loop(input_variables, config, None).await
    }

    /// Resume from a checkpoint with human decisions. Requires checkpointer and same thread_id.
    pub async fn call_resume(
        &self,
        config: &crate::graph::RunnableConfig,
        resume_decisions: serde_json::Value,
    ) -> Result<GenerateResult, ChainError> {
        let thread_id = config
            .get_thread_id()
            .ok_or_else(|| ChainError::OtherError("thread_id required for resume".to_string()))?;
        let state = match self.checkpointer.as_ref() {
            Some(cp) => cp.get_async(&thread_id).await,
            None => None,
        }
        .ok_or_else(|| {
            ChainError::OtherError(
                "No checkpoint found for thread (use same thread_id as interrupt)".to_string(),
            )
        })?;
        self.run_loop(
            state.input_variables.clone(),
            Some(config),
            Some((state, resume_decisions)),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use async_trait::async_trait;

    use super::*;
    use crate::agent::{Agent as AgentTrait, AgentCheckpointState, AgentCheckpointer};
    use crate::graph::RunnableConfig;
    use crate::schemas::agent::{AgentAction, AgentEvent, AgentFinish};

    #[test]
    fn test_convert_messages_to_prompt_args() {
        let mut input_vars = PromptArgs::new();
        input_vars.insert(
            "messages".to_string(),
            json!([
                {"message_type": "human", "content": "Hello"},
                {"message_type": "ai", "content": "Hi there!"}
            ]),
        );

        let result = convert_messages_to_prompt_args(input_vars);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert!(args.contains_key("input"));
        assert!(args.contains_key("chat_history"));
        assert_eq!(args["input"], json!("Hello"));
    }

    #[test]
    fn test_convert_messages_preserves_other_keys() {
        let mut input_vars = PromptArgs::new();
        input_vars.insert(
            "messages".to_string(),
            json!([{"message_type": "human", "content": "Hello"}]),
        );
        input_vars.insert("custom_key".to_string(), json!("custom_value"));

        let result = convert_messages_to_prompt_args(input_vars);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert!(args.contains_key("custom_key"));
        assert_eq!(args["custom_key"], json!("custom_value"));
    }

    struct FinishAgent;

    #[async_trait]
    impl AgentTrait for FinishAgent {
        async fn plan(
            &self,
            _intermediate_steps: &[(AgentAction, String)],
            _inputs: PromptArgs,
        ) -> Result<AgentEvent, crate::agent::AgentError> {
            Ok(AgentEvent::Finish(AgentFinish {
                output: "resumed".to_string(),
            }))
        }

        fn get_tools(&self) -> Vec<Arc<dyn crate::tools::Tool>> {
            Vec::new()
        }
    }

    struct AsyncResumeProbe {
        async_get_calls: AtomicUsize,
        sync_get_calls: AtomicUsize,
        stored: AgentCheckpointState,
    }

    impl AsyncResumeProbe {
        fn new(stored: AgentCheckpointState) -> Self {
            Self {
                async_get_calls: AtomicUsize::new(0),
                sync_get_calls: AtomicUsize::new(0),
                stored,
            }
        }
    }

    #[async_trait]
    impl AgentCheckpointer for AsyncResumeProbe {
        fn put(&self, _thread_id: &str, _state: &AgentCheckpointState) {}

        fn get(&self, _thread_id: &str) -> Option<AgentCheckpointState> {
            self.sync_get_calls.fetch_add(1, Ordering::SeqCst);
            None
        }

        async fn get_async(&self, _thread_id: &str) -> Option<AgentCheckpointState> {
            self.async_get_calls.fetch_add(1, Ordering::SeqCst);
            Some(self.stored.clone())
        }
    }

    #[tokio::test]
    async fn call_resume_uses_async_checkpoint_loader() {
        let mut input = PromptArgs::new();
        input.insert("input".to_string(), json!("resume request"));

        let checkpoint = AgentCheckpointState {
            steps: Vec::new(),
            input_variables: input,
            pending_actions: Vec::new(),
        };
        let checkpointer = Arc::new(AsyncResumeProbe::new(checkpoint));
        let executor =
            AgentExecutor::from_agent(FinishAgent).with_checkpointer(Some(checkpointer.clone()));

        let result = executor
            .call_resume(
                &RunnableConfig::with_thread_id("resume-thread"),
                json!({"decisions": []}),
            )
            .await
            .expect("resume");

        assert_eq!(result.generation, "resumed");
        assert_eq!(checkpointer.async_get_calls.load(Ordering::SeqCst), 1);
        assert_eq!(checkpointer.sync_get_calls.load(Ordering::SeqCst), 0);
    }
}
