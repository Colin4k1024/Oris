use async_trait::async_trait;

use crate::{agent::AgentError, language_models::llm::LLM};

/// Strategy for routing input to agents
pub enum RoutingStrategy {
    /// Use LLM to classify input and route to appropriate agent
    LLMBased {
        /// The LLM to use for routing
        llm: Box<dyn LLM>,
        /// Agent descriptions for the LLM to choose from
        agent_descriptions: Vec<(String, String)>,
    },
    /// Use keyword matching to route
    KeywordBased {
        /// Map of keywords to agent names
        keyword_map: std::collections::HashMap<String, Vec<String>>,
    },
}

/// Router that determines which agent should handle a given input
#[async_trait]
pub trait Router: Send + Sync {
    /// Route the input to determine which agent should handle it
    ///
    /// Returns the name of the agent that should handle the input,
    /// or None if no suitable agent is found.
    async fn route(&self, input: &str) -> Result<Option<String>, AgentError>;
}

/// Default router implementation
pub struct DefaultRouter {
    strategy: RoutingStrategy,
}

impl DefaultRouter {
    /// Create a new router with LLM-based routing
    pub fn with_llm(llm: Box<dyn LLM>, agent_descriptions: Vec<(String, String)>) -> Self {
        Self {
            strategy: RoutingStrategy::LLMBased {
                llm,
                agent_descriptions,
            },
        }
    }

    /// Create a new router with keyword-based routing
    pub fn with_keywords(keyword_map: std::collections::HashMap<String, Vec<String>>) -> Self {
        Self {
            strategy: RoutingStrategy::KeywordBased { keyword_map },
        }
    }
}

#[async_trait]
impl Router for DefaultRouter {
    async fn route(&self, input: &str) -> Result<Option<String>, AgentError> {
        match &self.strategy {
            RoutingStrategy::LLMBased {
                llm,
                agent_descriptions,
            } => {
                // Build a prompt for the LLM to classify the input
                let agent_list: Vec<String> = agent_descriptions
                    .iter()
                    .enumerate()
                    .map(|(i, (name, desc))| format!("{}. {}: {}", i + 1, name, desc))
                    .collect();

                let prompt = format!(
                    "You are a routing system. Based on the user input, determine which specialized agent should handle it.\n\n\
                    Available agents:\n{}\n\n\
                    User input: {}\n\n\
                    Respond with ONLY the agent name (exactly as listed above), or 'none' if no agent is suitable.",
                    agent_list.join("\n"),
                    input
                );

                let result = llm.invoke(&prompt).await?;
                let result = result.trim().to_lowercase();

                // Find matching agent name
                for (name, _) in agent_descriptions {
                    if result.contains(&name.to_lowercase()) {
                        return Ok(Some(name.clone()));
                    }
                }

                if result.contains("none") || result.is_empty() {
                    Ok(None)
                } else {
                    // Try to extract agent name from response
                    for (name, _) in agent_descriptions {
                        if result == name.to_lowercase() {
                            return Ok(Some(name.clone()));
                        }
                    }
                    Ok(None)
                }
            }
            RoutingStrategy::KeywordBased { keyword_map } => {
                let input_lower = input.to_lowercase();
                for (agent_name, keywords) in keyword_map {
                    for keyword in keywords {
                        if input_lower.contains(&keyword.to_lowercase()) {
                            return Ok(Some(agent_name.clone()));
                        }
                    }
                }
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_keyword_router_creation() {
        let mut keyword_map = HashMap::new();
        keyword_map.insert("agent1".to_string(), vec!["test".to_string()]);
        let _router = DefaultRouter::with_keywords(keyword_map);
        // Router created successfully
        assert!(true);
    }

    #[tokio::test]
    async fn test_keyword_router_routing() {
        let mut keyword_map = HashMap::new();
        keyword_map.insert(
            "weather".to_string(),
            vec!["weather".to_string(), "temperature".to_string()],
        );
        keyword_map.insert(
            "news".to_string(),
            vec!["news".to_string(), "headlines".to_string()],
        );

        let router = DefaultRouter::with_keywords(keyword_map);

        // Test weather routing
        let result = router.route("What's the weather like?").await.unwrap();
        assert_eq!(result, Some("weather".to_string()));

        // Test news routing
        let result = router.route("What are the latest news?").await.unwrap();
        assert_eq!(result, Some("news".to_string()));

        // Test no match
        let result = router.route("Random question").await.unwrap();
        assert_eq!(result, None);
    }
}
