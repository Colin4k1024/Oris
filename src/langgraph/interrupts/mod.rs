pub mod command;
pub mod context;
pub mod error;
pub mod result;
mod state_or_command;
pub mod types;

#[cfg(test)]
mod tests;

pub use command::*;
pub use context::*;
pub use error::*;
pub use result::*;
pub use state_or_command::*;
pub use types::*;

/// Interrupt execution and wait for external input
///
/// This function pauses graph execution at the point where it's called.
/// The value passed to `interrupt()` is returned to the caller in the
/// `__interrupt__` field of the result.
///
/// When execution is resumed with `Command::resume(value)`, that value
/// becomes the return value of this function call.
///
/// # Arguments
///
/// * `value` - Any JSON-serializable value to pass to the caller
///
/// # Returns
///
/// * `Ok(resume_value)` - When resumed, returns the resume value
/// * `Err(InterruptError)` - When first called, triggers an interrupt
///
/// # Example
///
/// ```rust,no_run
/// use oris::langgraph::interrupts::interrupt;
/// use oris::langgraph::error::LangGraphError;
///
/// async fn approval_node(state: &MessagesState) -> Result<StateUpdate, LangGraphError> {
///     let approved = interrupt("Do you approve this action?").await
///         .map_err(|e| LangGraphError::InterruptError(e))?;
///     // When resumed, `approved` contains the resume value
///     Ok(/* ... */)
/// }
/// ```
pub async fn interrupt(
    value: impl Into<serde_json::Value>,
) -> Result<serde_json::Value, InterruptError> {
    use context::INTERRUPT_CONTEXT;

    let value = value.into();

    // Get the current context
    let resume_value = INTERRUPT_CONTEXT.with(|ctx| {
        let ctx = ctx.borrow();
        if let Some(ref c) = *ctx {
            // Check if we have a resume value for the current index
            if c.current_index < c.resume_values.len() {
                Some((c.resume_values[c.current_index].clone(), c.current_index))
            } else {
                None
            }
        } else {
            None
        }
    });

    if let Some((resume, index)) = resume_value {
        // We have a resume value, return it and increment index
        INTERRUPT_CONTEXT.with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            if let Some(ref mut c) = *ctx {
                c.current_index = index + 1;
            }
        });
        Ok(resume)
    } else {
        // No resume value, set interrupt value and trigger interrupt
        INTERRUPT_CONTEXT.with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            if let Some(ref mut c) = *ctx {
                c.interrupt_value = Some(value.clone());
            } else {
                // Create new context if it doesn't exist
                *ctx = Some(InterruptContext {
                    interrupt_value: Some(value.clone()),
                    resume_values: Vec::new(),
                    current_index: 0,
                });
            }
        });
        Err(InterruptError::new(value))
    }
}
