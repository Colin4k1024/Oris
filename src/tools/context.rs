/// Immutable context information available to tools.
///
/// Context provides access to configuration, user IDs, session details,
/// and other immutable information that doesn't change during execution.
pub trait ToolContext: Send + Sync {
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

/// Empty context implementation for when no context is needed.
#[derive(Clone, Debug)]
pub struct EmptyContext;

impl ToolContext for EmptyContext {}

impl Default for EmptyContext {
    fn default() -> Self {
        Self
    }
}

/// Simple context implementation with user and session IDs.
#[derive(Clone, Debug)]
pub struct SimpleContext {
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub custom: std::collections::HashMap<String, String>,
}

impl SimpleContext {
    pub fn new() -> Self {
        Self {
            user_id: None,
            session_id: None,
            custom: std::collections::HashMap::new(),
        }
    }

    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn with_custom(mut self, key: String, value: String) -> Self {
        self.custom.insert(key, value);
        self
    }
}

impl Default for SimpleContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolContext for SimpleContext {
    fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.custom.get(key).map(|s| s.as_str())
    }
}
