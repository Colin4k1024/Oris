use std::sync::Arc;

use crate::tools::ToolContext;

/// Trait for type-safe context definitions.
///
/// This allows users to define structured context types (similar to Python's dataclass)
/// that can be automatically converted to `ToolContext` for use in agents.
pub trait TypedContext: Send + Sync + Clone {
    /// Convert this typed context to a ToolContext
    fn to_tool_context(&self) -> Arc<dyn ToolContext>;
}

/// Adapter that wraps a typed context and implements ToolContext.
///
/// This allows any type implementing TypedContext to be used as a ToolContext.
pub struct ContextAdapter<C: TypedContext> {
    context: C,
}

impl<C: TypedContext> ContextAdapter<C> {
    /// Create a new ContextAdapter
    pub fn new(context: C) -> Self {
        Self { context }
    }
}

/// Helper trait for TypedContext to provide common field access.
///
/// This allows TypedContext implementations to provide user_id, session_id, etc.
/// Users should implement this trait for their context types to enable field access.
pub trait TypedContextFields: TypedContext {
    /// Get the user ID if available
    fn user_id(&self) -> Option<&str> {
        None
    }

    /// Get the session ID if available
    fn session_id(&self) -> Option<&str> {
        None
    }

    /// Get a custom context value by key
    fn get(&self, _key: &str) -> Option<&str> {
        None
    }
}

// Implementation for TypedContext with TypedContextFields
impl<C> ToolContext for ContextAdapter<C>
where
    C: TypedContextFields,
{
    fn user_id(&self) -> Option<&str> {
        self.context.user_id()
    }

    fn session_id(&self) -> Option<&str> {
        self.context.session_id()
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.context.get(key)
    }
}

/// Macro to simplify creating TypedContext implementations.
///
/// This macro generates the necessary implementations for a struct to work as a TypedContext.
#[macro_export]
macro_rules! define_typed_context {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident: $field_type:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: $field_type
            ),*
        }

        impl $crate::agent::runtime::TypedContext for $name {
            fn to_tool_context(&self) -> std::sync::Arc<dyn $crate::tools::ToolContext> {
                std::sync::Arc::new($crate::agent::runtime::ContextAdapter::new(self.clone()))
            }
        }

        impl $crate::agent::runtime::TypedContextFields for $name {
            fn user_id(&self) -> Option<&str> {
                // Try to access user_id field if it exists
                // This is a limitation - we can't use reflection in stable Rust
                // Users should implement TypedContextFields manually for custom access
                None
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestContext {
        user_id: String,
        user_name: String,
    }

    impl TypedContext for TestContext {
        fn to_tool_context(&self) -> Arc<dyn ToolContext> {
            Arc::new(ContextAdapter::new(self.clone()))
        }
    }

    impl TypedContextFields for TestContext {
        fn user_id(&self) -> Option<&str> {
            Some(&self.user_id)
        }
    }

    #[test]
    fn test_typed_context_conversion() {
        let ctx = TestContext {
            user_id: "user123".to_string(),
            user_name: "John".to_string(),
        };

        let tool_ctx = ctx.to_tool_context();
        assert_eq!(tool_ctx.user_id(), Some("user123"));
    }
}
