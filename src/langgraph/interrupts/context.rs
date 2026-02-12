use serde_json::Value;
use tokio::task_local;

// Task-local interrupt context: passes interrupt state and resume values
// through the async execution stack, similar to Python's context vars.
task_local! {
    pub static INTERRUPT_CONTEXT: std::cell::RefCell<Option<InterruptContext>>;
}

/// Interrupt context for managing interrupt state
///
/// This context tracks:
/// - The current interrupt value (if an interrupt occurred)
/// - Resume values for each interrupt call
/// - The current index for matching resume values to interrupt calls
#[derive(Clone, Debug)]
pub struct InterruptContext {
    /// The current interrupt value (set when interrupt() is called)
    pub interrupt_value: Option<Value>,
    /// Resume values for interrupt calls (in order)
    pub resume_values: Vec<Value>,
    /// Current index for matching resume values
    pub current_index: usize,
}

impl InterruptContext {
    /// Create a new empty interrupt context
    pub fn new() -> Self {
        Self {
            interrupt_value: None,
            resume_values: Vec::new(),
            current_index: 0,
        }
    }

    /// Create a context with a single resume value
    pub fn with_resume_value(value: Value) -> Self {
        Self {
            interrupt_value: None,
            resume_values: vec![value],
            current_index: 0,
        }
    }

    /// Create a context with multiple resume values
    pub fn with_resume_values(values: Vec<Value>) -> Self {
        Self {
            interrupt_value: None,
            resume_values: values,
            current_index: 0,
        }
    }

    /// Check if there's an active interrupt
    pub fn has_interrupt(&self) -> bool {
        self.interrupt_value.is_some()
    }

    /// Get the interrupt value
    pub fn interrupt_value(&self) -> Option<&Value> {
        self.interrupt_value.as_ref()
    }

    /// Reset the context for a new execution
    pub fn reset(&mut self) {
        self.interrupt_value = None;
        self.current_index = 0;
    }
}

impl Default for InterruptContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Set the interrupt context for the current task
pub async fn set_interrupt_context<F, R>(context: InterruptContext, f: F) -> R
where
    F: std::future::Future<Output = R>,
{
    INTERRUPT_CONTEXT
        .scope(std::cell::RefCell::new(Some(context)), f)
        .await
}

/// Get the current interrupt value from context
pub fn get_interrupt_value() -> Option<Value> {
    INTERRUPT_CONTEXT
        .try_with(|ctx| {
            ctx.borrow()
                .as_ref()
                .and_then(|c| c.interrupt_value.clone())
        })
        .ok()
        .flatten()
}

/// Check if there's an active interrupt in the context
pub fn has_interrupt() -> bool {
    INTERRUPT_CONTEXT
        .try_with(|ctx| {
            ctx.borrow()
                .as_ref()
                .map(|c| c.has_interrupt())
                .unwrap_or(false)
        })
        .ok()
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_interrupt_context() {
        let ctx = InterruptContext::with_resume_value(serde_json::json!("resume"));

        set_interrupt_context(ctx, async {
            let value = get_interrupt_value();
            assert_eq!(value, None);
        })
        .await;
    }
}
