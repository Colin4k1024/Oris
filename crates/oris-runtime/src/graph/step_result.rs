//! Result of running a single graph step (one node).
//! Used by GraphStepFnAdapter to drive the kernel step-by-step.

use serde_json::Value;

use crate::graph::state::State;

/// Result of executing one node in the graph.
#[derive(Debug, Clone)]
pub enum GraphStepOnceResult<S: State> {
    /// Node produced state update; run next node (may be END).
    Emit {
        /// Node that was just executed (for audit / step_id).
        executed_node: String,
        new_state: S,
        next_node: String,
    },
    /// Interrupt reached (e.g. human-in-the-loop).
    Interrupt {
        state: S,
        value: Value,
    },
    /// Graph reached END.
    Complete { state: S },
}
