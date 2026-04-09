//! API request types.

use serde::Deserialize;

/// Query parameters for fetching experiences.
#[derive(Debug, Clone, Deserialize)]
pub struct FetchQuery {
    /// Comma-separated problem signals (e.g., "timeout,error")
    #[serde(default)]
    pub q: Option<String>,

    /// Minimum confidence threshold (default: 0.5)
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f64,

    /// Maximum number of results (default: 10)
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Pagination cursor
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_min_confidence() -> f64 {
    0.5
}

fn default_limit() -> usize {
    10
}

impl FetchQuery {
    /// Parse the query string into signals.
    pub fn signals(&self) -> Vec<String> {
        self.q
            .as_ref()
            .map(|q| {
                q.split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_query_signals() {
        let query = FetchQuery {
            q: Some("timeout,error".to_string()),
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let signals = query.signals();
        assert_eq!(signals, vec!["timeout", "error"]);
    }

    #[test]
    fn test_fetch_query_signals_with_spaces() {
        let query = FetchQuery {
            q: Some(" timeout , error , memory ".to_string()),
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let signals = query.signals();
        assert_eq!(signals, vec!["timeout", "error", "memory"]);
    }

    #[test]
    fn test_fetch_query_signals_empty() {
        let query = FetchQuery {
            q: None,
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let signals = query.signals();
        assert!(signals.is_empty());
    }
}
