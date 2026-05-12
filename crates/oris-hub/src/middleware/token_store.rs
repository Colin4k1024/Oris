use std::collections::HashSet;
use std::sync::RwLock;

pub struct TokenStore {
    tokens: RwLock<HashSet<String>>,
}

impl TokenStore {
    pub fn new() -> Self {
        Self {
            tokens: RwLock::new(HashSet::new()),
        }
    }

    pub fn with_tokens(tokens: Vec<String>) -> Self {
        let set: HashSet<String> = tokens.into_iter().collect();
        Self {
            tokens: RwLock::new(set),
        }
    }

    pub fn add_token(&self, token: &str) {
        if let Ok(mut set) = self.tokens.write() {
            set.insert(token.to_string());
        }
    }

    pub fn revoke_token(&self, token: &str) {
        if let Ok(mut set) = self.tokens.write() {
            set.remove(token);
        }
    }

    pub fn validate(&self, token: &str) -> bool {
        self.tokens
            .read()
            .map(|set| set.contains(token))
            .unwrap_or(false)
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}
