use crate::langgraph::state::State;

use super::Command;

/// Helper type to accept either State or Command as input
///
/// This allows `invoke_with_config_interrupt` to accept either
/// a regular state or a Command for resuming execution.
#[derive(Debug)]
pub enum StateOrCommand<S: State> {
    State(S),
    Command(Command),
}

impl<S: State> From<S> for StateOrCommand<S> {
    fn from(state: S) -> Self {
        Self::State(state)
    }
}

impl<S: State> From<Command> for StateOrCommand<S> {
    fn from(cmd: Command) -> Self {
        Self::Command(cmd)
    }
}
