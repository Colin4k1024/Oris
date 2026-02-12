// Example: Middleware accessing runtime
// This demonstrates how middleware can access runtime context, store, and stream writer

use std::sync::Arc;

use oris::{
    agent::{
        create_agent, Middleware, MiddlewareContext, MiddlewareError, Runtime, RuntimeRequest,
    },
    language_models::GenerateResult,
    prompt::PromptArgs,
    schemas::agent::{AgentAction, AgentFinish},
    schemas::Message,
};

// Custom middleware that uses runtime
struct RuntimeAwareMiddleware;

#[async_trait::async_trait]
impl Middleware for RuntimeAwareMiddleware {
    async fn before_agent_plan_with_runtime(
        &self,
        request: &RuntimeRequest,
        _steps: &[(AgentAction, String)],
        _context: &mut MiddlewareContext,
    ) -> Result<Option<PromptArgs>, MiddlewareError> {
        // Access runtime context
        if let Some(runtime) = request.runtime() {
            if let Some(user_id) = runtime.context().user_id() {
                println!("Processing request for user: {}", user_id);

                // Modify input to include user context
                let mut modified_input = request.input.clone();
                modified_input.insert(
                    "user_context".to_string(),
                    serde_json::json!({
                        "user_id": user_id,
                    }),
                );

                return Ok(Some(modified_input));
            }
        }

        Ok(None)
    }

    async fn after_finish_with_runtime(
        &self,
        _finish: &AgentFinish,
        _result: &GenerateResult,
        runtime: Option<&Runtime>,
        _context: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        // Log using runtime context
        if let Some(runtime) = runtime {
            if let Some(user_id) = runtime.context().user_id() {
                println!("Completed request for user: {}", user_id);
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create middleware
    let middleware: Vec<Arc<dyn Middleware>> = vec![Arc::new(RuntimeAwareMiddleware)];

    // Create agent with middleware
    let agent = create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a helpful assistant"),
        Some(middleware),
    )?;

    // Use the agent
    let result = agent
        .invoke_messages(vec![Message::new_human_message("Hello")])
        .await?;

    println!("Agent response: {}", result);

    Ok(())
}
