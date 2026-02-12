use regex::Regex;
use std::collections::HashMap;

/// Helper utilities for creating custom guardrails.

/// Extract text content from various message formats.
pub fn extract_text_from_messages(messages: &[serde_json::Value]) -> String {
    messages
        .iter()
        .filter_map(|msg| {
            msg.get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if text matches any of the given patterns.
pub fn matches_patterns(text: &str, patterns: &[Regex]) -> Option<usize> {
    for (idx, pattern) in patterns.iter().enumerate() {
        if pattern.is_match(text) {
            return Some(idx);
        }
    }
    None
}

/// Count occurrences of keywords in text.
pub fn count_keywords(
    text: &str,
    keywords: &[String],
    case_sensitive: bool,
) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    let search_text = if case_sensitive {
        text.to_string()
    } else {
        text.to_lowercase()
    };

    for keyword in keywords {
        let search_keyword = if case_sensitive {
            keyword.clone()
        } else {
            keyword.to_lowercase()
        };

        let count = search_text.matches(&search_keyword).count();
        if count > 0 {
            counts.insert(keyword.clone(), count);
        }
    }

    counts
}

/// Validate text length against thresholds.
pub fn validate_length(text: &str, min: Option<usize>, max: Option<usize>) -> bool {
    let len = text.len();

    if let Some(min_len) = min {
        if len < min_len {
            return false;
        }
    }

    if let Some(max_len) = max {
        if len > max_len {
            return false;
        }
    }

    true
}

/// Check if text contains any URLs.
pub fn contains_urls(text: &str) -> bool {
    let url_pattern = Regex::new(r"https?://[^\s]+").unwrap();
    url_pattern.is_match(text)
}

/// Extract all URLs from text.
pub fn extract_urls(text: &str) -> Vec<String> {
    let url_pattern = Regex::new(r"https?://[^\s]+").unwrap();
    url_pattern
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Check if text contains profanity (basic word list).
pub fn contains_profanity(text: &str) -> bool {
    // Basic profanity list - in production, use a comprehensive library
    let profanity_words = vec![
        "damn", "hell", "crap", // Add more as needed
    ];

    let text_lower = text.to_lowercase();
    profanity_words.iter().any(|word| text_lower.contains(word))
}

/// Calculate text similarity using simple word overlap.
///
/// This is a re-export of the unified text_similarity function from utils.
pub fn text_similarity(text1: &str, text2: &str) -> f64 {
    crate::utils::text_similarity(text1, text2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_urls() {
        let text = "Visit https://example.com and http://test.org for more info";
        let urls = extract_urls(text);
        assert_eq!(urls.len(), 2);
        assert!(urls.iter().any(|u| u.contains("example.com")));
    }

    #[test]
    fn test_contains_urls() {
        assert!(contains_urls("Check https://example.com"));
        assert!(!contains_urls("No URLs here"));
    }

    #[test]
    fn test_count_keywords() {
        let text = "hello world hello";
        let keywords = vec!["hello".to_string(), "world".to_string()];
        let counts = count_keywords(text, &keywords, false);

        assert_eq!(counts.get("hello"), Some(&2));
        assert_eq!(counts.get("world"), Some(&1));
    }

    #[test]
    fn test_validate_length() {
        assert!(validate_length("hello", Some(3), Some(10)));
        assert!(!validate_length("hi", Some(3), Some(10)));
        assert!(!validate_length("this is too long", Some(3), Some(10)));
    }

    #[test]
    fn test_text_similarity() {
        let text1 = "hello world";
        let text2 = "hello world";
        assert!((text_similarity(text1, text2) - 1.0).abs() < 0.001);

        let text3 = "hello";
        assert!(text_similarity(text1, text3) < 1.0);
    }
}
