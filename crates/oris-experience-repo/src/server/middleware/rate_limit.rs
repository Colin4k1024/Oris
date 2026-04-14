//! Rate limiting middleware.
//!
//! This module provides a simple sliding window rate limiting mechanism.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};

use axum::{
    body::Body,
    http::{Method, StatusCode},
    response::Response,
};

/// Rate limit configuration per endpoint.
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Requests per minute for GET /experience
    pub get_experience_rpm: NonZeroU32,
    /// Requests per minute for POST /experience
    pub post_experience_rpm: NonZeroU32,
    /// Requests per minute for key management endpoints
    pub key_management_rpm: NonZeroU32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            get_experience_rpm: NonZeroU32::new(100).unwrap(),
            post_experience_rpm: NonZeroU32::new(30).unwrap(),
            key_management_rpm: NonZeroU32::new(20).unwrap(),
        }
    }
}

/// Endpoint rate limiter with sliding window.
#[derive(Clone)]
pub struct EndpointLimiter {
    requests: Arc<RwLock<HashMap<String, Vec<u64>>>>,
    max_requests: NonZeroU32,
    window_secs: u64,
}

impl EndpointLimiter {
    fn new(max_requests: NonZeroU32, window_secs: u64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window_secs,
        }
    }

    fn check(&self, key: &str, now_secs: u64) -> Result<(), u64> {
        let window = self.window_secs;
        let cutoff = now_secs.saturating_sub(window);

        let mut requests = self.requests.write().unwrap();

        // Clean old entries
        if let Some(timestamps) = requests.get_mut(key) {
            timestamps.retain(|&t| t > cutoff);
        }

        // Check limit
        let count = requests.get(key).map(|v| v.len()).unwrap_or(0);
        if count >= self.max_requests.get() as usize {
            // Calculate retry after based on oldest request
            if let Some(timestamps) = requests.get(key) {
                if let Some(&oldest) = timestamps.first() {
                    let retry_after = oldest + window - now_secs;
                    return Err(retry_after.max(1));
                }
            }
            return Err(window);
        }

        // Record request
        requests
            .entry(key.to_string())
            .or_insert_with(Vec::new)
            .push(now_secs);
        Ok(())
    }
}

/// Rate limiter registry managing multiple endpoint limiters.
#[derive(Clone)]
pub struct RateLimiterRegistry {
    get_experience: EndpointLimiter,
    post_experience: EndpointLimiter,
    key_management: EndpointLimiter,
}

impl RateLimiterRegistry {
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            get_experience: EndpointLimiter::new(config.get_experience_rpm, 60),
            post_experience: EndpointLimiter::new(config.post_experience_rpm, 60),
            key_management: EndpointLimiter::new(config.key_management_rpm, 60),
        }
    }

    /// Get the appropriate rate limiter for a request.
    pub fn get_limiter(&self, method: &Method, path: &str) -> Option<&EndpointLimiter> {
        if path.starts_with("/experience") && method == Method::GET {
            Some(&self.get_experience)
        } else if path.starts_with("/experience") && method == Method::POST {
            Some(&self.post_experience)
        } else if path.starts_with("/keys") || path.starts_with("/public-keys") {
            Some(&self.key_management)
        } else {
            None
        }
    }

    /// Check rate limit for a request. Returns Ok(()) if allowed, Err(retry_after_secs) if limited.
    pub fn check(
        &self,
        method: &Method,
        path: &str,
        client_id: &str,
        now_secs: u64,
    ) -> Result<(), u64> {
        match self.get_limiter(method, path) {
            Some(limiter) => limiter.check(client_id, now_secs),
            None => Ok(()),
        }
    }
}

/// Extract client identifier from request headers for rate limiting.
pub fn extract_client_id(request: &axum::http::Request<axum::body::Body>) -> String {
    // Try X-Forwarded-For first
    if let Some(forwarded) = request.headers().get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            if let Some(ip) = forwarded_str.split(',').next() {
                return ip.trim().to_string();
            }
        }
    }

    // Try X-Real-IP
    if let Some(real_ip) = request.headers().get("x-real-ip") {
        if let Ok(ip) = real_ip.to_str() {
            return ip.to_string();
        }
    }

    // Default
    "default".to_string()
}

/// Create a rate limit exceeded response.
pub fn rate_limit_response(retry_after_secs: u64) -> Response<Body> {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header("Retry-After", retry_after_secs.to_string())
        .header("Content-Type", "application/json")
        .body(Body::from(format!(
            r#"{{"error":"rate limit exceeded","error_code":"RATE_LIMIT_EXCEEDED","retry_after":{}}}"#,
            retry_after_secs
        )))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RateLimitConfig::default();
        assert_eq!(config.get_experience_rpm.get(), 100);
        assert_eq!(config.post_experience_rpm.get(), 30);
        assert_eq!(config.key_management_rpm.get(), 20);
    }

    #[test]
    fn test_rate_limiter_allows_within_limit() {
        let limiter = EndpointLimiter::new(NonZeroU32::new(5).unwrap(), 60);
        let now = 1000u64;

        // Should allow 5 requests
        for i in 0..5 {
            let result = limiter.check(&format!("client-{}", i), now);
            assert!(result.is_ok(), "request {} should be allowed", i);
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let limiter = EndpointLimiter::new(NonZeroU32::new(2).unwrap(), 60);
        let now = 1000u64;

        // Allow 2 requests
        assert!(limiter.check("client", now).is_ok());
        assert!(limiter.check("client", now).is_ok());

        // Block third request
        assert!(limiter.check("client", now).is_err());
    }
}
