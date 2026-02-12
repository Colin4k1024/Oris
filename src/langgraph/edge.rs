use std::collections::HashMap;

use std::sync::Arc;

use super::{error::LangGraphError, state::State};

/// Special node names for graph entry and exit
pub const START: &str = "__start__";
pub const END: &str = "__end__";

/// Edge type - either a regular edge or a conditional edge
#[derive(Clone)]
pub enum EdgeType<S: State> {
    /// Regular edge - fixed routing to a single node
    Regular { to: String },
    /// Conditional edge - dynamic routing based on state
    Conditional {
        condition: Arc<
            dyn Fn(
                    &S,
                ) -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<String, LangGraphError>> + Send>,
                > + Send
                + Sync,
        >,
        mapping: HashMap<String, String>, // Maps condition result to node name
    },
}

impl<S: State> std::fmt::Debug for EdgeType<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeType::Regular { to } => f.debug_struct("Regular").field("to", to).finish(),
            EdgeType::Conditional { mapping, .. } => f
                .debug_struct("Conditional")
                .field("condition", &"<fn>")
                .field("mapping", mapping)
                .finish(),
        }
    }
}

/// Edge in the graph
#[derive(Clone, Debug)]
pub struct Edge<S: State> {
    pub from: String,
    pub edge_type: EdgeType<S>,
}

impl<S: State> Edge<S> {
    /// Create a new regular edge
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            edge_type: EdgeType::Regular { to: to.into() },
        }
    }

    /// Create a new conditional edge
    pub fn conditional<F, Fut>(
        from: impl Into<String>,
        condition: F,
        mapping: HashMap<String, String>,
    ) -> Self
    where
        F: Fn(&S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String, LangGraphError>> + Send + 'static,
    {
        Self {
            from: from.into(),
            edge_type: EdgeType::Conditional {
                condition: Arc::new(move |state| Box::pin(condition(state))),
                mapping,
            },
        }
    }

    /// Get the target node name for a given state
    ///
    /// For regular edges, this always returns the same node.
    /// For conditional edges, this evaluates the condition function.
    pub async fn get_target(&self, state: &S) -> Result<String, LangGraphError> {
        match &self.edge_type {
            EdgeType::Regular { to } => Ok(to.clone()),
            EdgeType::Conditional { condition, mapping } => {
                let condition_result = (condition)(state).await?;
                mapping.get(&condition_result).cloned().ok_or_else(|| {
                    LangGraphError::ConditionError(format!(
                        "Condition returned '{}' which is not in mapping",
                        condition_result
                    ))
                })
            }
        }
    }

    /// Check if this is a regular edge
    pub fn is_regular(&self) -> bool {
        matches!(self.edge_type, EdgeType::Regular { .. })
    }

    /// Check if this is a conditional edge
    pub fn is_conditional(&self) -> bool {
        matches!(self.edge_type, EdgeType::Conditional { .. })
    }
}

/// Helper function to create a regular edge
pub fn edge<S: State>(from: impl Into<String>, to: impl Into<String>) -> Edge<S> {
    Edge::new(from, to)
}

/// Helper function to create a conditional edge
pub fn conditional_edge<S: State, F, Fut>(
    from: impl Into<String>,
    condition: F,
    mapping: HashMap<String, String>,
) -> Edge<S>
where
    F: Fn(&S) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<String, LangGraphError>> + Send + 'static,
{
    Edge::conditional(from, condition, mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langgraph::state::MessagesState;

    #[tokio::test]
    async fn test_regular_edge() {
        let edge = Edge::new("node1", "node2");
        let state = MessagesState::new();
        let target = edge.get_target(&state).await.unwrap();
        assert_eq!(target, "node2");
        assert!(edge.is_regular());
    }

    #[tokio::test]
    async fn test_conditional_edge() {
        let mut mapping = HashMap::new();
        mapping.insert("yes".to_string(), "node_yes".to_string());
        mapping.insert("no".to_string(), "node_no".to_string());

        let edge = Edge::conditional(
            "node1",
            |_state: &MessagesState| async move { Ok("yes".to_string()) },
            mapping,
        );

        let state = MessagesState::new();
        let target = edge.get_target(&state).await.unwrap();
        assert_eq!(target, "node_yes");
        assert!(edge.is_conditional());
    }
}
