use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    agent::{
        context_engineering::ModelRequest,
        middleware::{Middleware, MiddlewareContext, MiddlewareError},
    },
    schemas::Message,
};

/// Position where messages should be injected
#[derive(Debug, Clone, Copy)]
pub enum InjectionPosition {
    /// Inject at the beginning of the message list
    Beginning,
    /// Inject before the last message
    BeforeLast,
    /// Inject at the end of the message list
    End,
}

/// Middleware that injects additional messages into the model request.
///
/// This allows you to inject context like file information, compliance rules,
/// or writing style guides based on State, Store, or Runtime Context.
pub struct MessageInjectionMiddleware {
    /// Function that generates messages to inject based on ModelRequest
    injector: Arc<dyn Fn(&ModelRequest) -> Vec<Message> + Send + Sync>,
    /// Position where messages should be injected
    position: InjectionPosition,
}

impl MessageInjectionMiddleware {
    /// Create a new MessageInjectionMiddleware with an injector function
    pub fn new<F>(injector: F, position: InjectionPosition) -> Self
    where
        F: Fn(&ModelRequest) -> Vec<Message> + Send + Sync + 'static,
    {
        Self {
            injector: Arc::new(injector),
            position,
        }
    }

    /// Create an injector that adds file context from State
    pub fn inject_file_context(position: InjectionPosition) -> Self {
        Self::new(
            move |request: &ModelRequest| {
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    let state = handle.block_on(request.state());

                    // Check for uploaded files in state
                    if let Some(files_value) = state.get_field("uploaded_files") {
                        if let Some(files) = files_value.as_array() {
                            let mut file_descriptions = Vec::new();
                            for file in files {
                                if let Some(file_obj) = file.as_object() {
                                    let name = file_obj
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let file_type = file_obj
                                        .get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let summary = file_obj
                                        .get("summary")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    file_descriptions
                                        .push(format!("- {} ({}): {}", name, file_type, summary));
                                }
                            }

                            if !file_descriptions.is_empty() {
                                let context = format!(
                                    "Files you have access to in this conversation:\n{}\n\nReference these files when answering questions.",
                                    file_descriptions.join("\n")
                                );

                                return vec![Message::new_human_message(&context)];
                            }
                        }
                    }
                }

                vec![]
            },
            position,
        )
    }

    /// Create an injector that adds compliance rules from Runtime Context
    pub fn inject_compliance_rules(position: InjectionPosition) -> Self {
        Self::new(
            move |request: &ModelRequest| {
                if let Some(runtime) = request.runtime() {
                    let mut rules = Vec::new();

                    // Check for compliance frameworks
                    if let Some(frameworks) = runtime.context().get("compliance_frameworks") {
                        // Parse frameworks (simplified - in practice would be more structured)
                        if frameworks.contains("GDPR") {
                            rules.push(
                                "- Must obtain explicit consent before processing personal data",
                            );
                            rules.push("- Users have right to data deletion");
                        }
                        if frameworks.contains("HIPAA") {
                            rules.push(
                                "- Cannot share patient health information without authorization",
                            );
                            rules.push("- Must use secure, encrypted communication");
                        }
                    }

                    // Check for industry
                    if let Some(industry) = runtime.context().get("industry") {
                        if industry == "finance" {
                            rules.push(
                                "- Cannot provide financial advice without proper disclaimers",
                            );
                        }
                    }

                    if !rules.is_empty() {
                        let jurisdiction = runtime
                            .context()
                            .get("user_jurisdiction")
                            .unwrap_or("default");
                        let context = format!(
                            "Compliance requirements for {}:\n{}",
                            jurisdiction,
                            rules.join("\n")
                        );

                        return vec![Message::new_human_message(&context)];
                    }
                }

                vec![]
            },
            position,
        )
    }

    /// Create an injector that adds writing style from Store
    pub fn inject_writing_style(position: InjectionPosition) -> Self {
        Self::new(
            move |request: &ModelRequest| {
                if let Some(runtime) = request.runtime() {
                    if let Some(_user_id) = runtime.context().user_id() {
                        // In a real implementation, you'd read from store:
                        // let writing_style = runtime.store().get(("writing_style",), user_id).await?;
                        // if let Some(style) = writing_style {
                        //     let context = format!("Your writing style: ...");
                        //     return vec![Message::new_human_message(&context)];
                        // }
                    }
                }

                vec![]
            },
            position,
        )
    }
}

#[async_trait]
impl Middleware for MessageInjectionMiddleware {
    async fn before_model_call(
        &self,
        request: &ModelRequest,
        _context: &mut MiddlewareContext,
    ) -> Result<Option<ModelRequest>, MiddlewareError> {
        // Generate messages to inject
        let messages_to_inject = (self.injector)(request);

        if messages_to_inject.is_empty() {
            return Ok(None);
        }

        // Inject messages at the specified position
        let mut messages = request.messages.clone();

        match self.position {
            InjectionPosition::Beginning => {
                // Insert at the beginning
                for msg in messages_to_inject.into_iter().rev() {
                    messages.insert(0, msg);
                }
            }
            InjectionPosition::BeforeLast => {
                // Insert before the last message
                if messages.len() > 0 {
                    let last = messages.pop().unwrap();
                    messages.extend(messages_to_inject);
                    messages.push(last);
                } else {
                    messages.extend(messages_to_inject);
                }
            }
            InjectionPosition::End => {
                // Append at the end
                messages.extend(messages_to_inject);
            }
        }

        Ok(Some(
            request.with_messages_and_tools(messages, request.tools.clone()),
        ))
    }
}

impl Clone for MessageInjectionMiddleware {
    fn clone(&self) -> Self {
        Self {
            injector: Arc::clone(&self.injector),
            position: self.position,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentState;
    use crate::schemas::Message;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_message_injection_beginning() {
        let middleware = MessageInjectionMiddleware::new(
            |_| vec![Message::new_human_message("Injected context")],
            InjectionPosition::Beginning,
        );

        let state = Arc::new(Mutex::new(AgentState::new()));
        let messages = vec![Message::new_human_message("Hello")];
        let request = ModelRequest::new(messages, vec![], state);

        let mut middleware_context = MiddlewareContext::new();
        let result = middleware
            .before_model_call(&request, &mut middleware_context)
            .await;

        assert!(result.is_ok());
        if let Ok(Some(modified)) = result {
            assert_eq!(modified.messages.len(), 2);
            assert_eq!(modified.messages[0].content, "Injected context");
        }
    }

    #[tokio::test]
    async fn test_message_injection_file_context() {
        let middleware = MessageInjectionMiddleware::inject_file_context(InjectionPosition::End);

        let mut state = AgentState::new();
        state.set_field(
            "uploaded_files".to_string(),
            json!([
                {
                    "name": "document.pdf",
                    "type": "pdf",
                    "summary": "Project proposal"
                }
            ]),
        );
        let state = Arc::new(Mutex::new(state));

        let messages = vec![Message::new_human_message("Hello")];
        let request = ModelRequest::new(messages, vec![], state);

        let mut middleware_context = MiddlewareContext::new();
        let result = middleware
            .before_model_call(&request, &mut middleware_context)
            .await;

        assert!(result.is_ok());
    }
}
