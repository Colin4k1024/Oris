//! Agent checkpointer for human-in-the-loop: persist state at interrupt and resume with decisions.

use serde::{Deserialize, Serialize};

use crate::prompt::PromptArgs;
use crate::schemas::agent::AgentAction;

/// Serializable agent state saved at interrupt.
///
/// Used to resume execution after human provides decisions for pending tool calls.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentCheckpointState {
    /// Steps (action, observation) so far.
    pub steps: Vec<(AgentAction, String)>,
    /// Last plan input (chat_history, input, etc.).
    pub input_variables: PromptArgs,
    /// Tool calls that were pending when we interrupted (same order as in interrupt payload).
    pub pending_actions: Vec<AgentAction>,
}

/// Trait for persisting and loading agent checkpoint state (e.g. for HILP resume).
pub trait AgentCheckpointer: Send + Sync {
    /// Save state for the given thread. Overwrites any existing state for that thread.
    fn put(&self, thread_id: &str, state: &AgentCheckpointState);

    /// Load state for the given thread, if any.
    fn get(&self, thread_id: &str) -> Option<AgentCheckpointState>;
}

/// In-memory checkpointer (one state per thread_id).
#[derive(Default)]
pub struct InMemoryAgentSaver {
    state: std::sync::RwLock<std::collections::HashMap<String, AgentCheckpointState>>,
}

impl InMemoryAgentSaver {
    pub fn new() -> Self {
        Self {
            state: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl AgentCheckpointer for InMemoryAgentSaver {
    fn put(&self, thread_id: &str, state: &AgentCheckpointState) {
        let mut g = self.state.write().expect("lock");
        g.insert(thread_id.to_string(), state.clone());
    }

    fn get(&self, thread_id: &str) -> Option<AgentCheckpointState> {
        let g = self.state.read().expect("lock");
        g.get(thread_id).cloned()
    }
}
