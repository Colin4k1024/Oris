use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerRateLimitConfig {
    pub max_capsules_per_hour: usize,
    pub window_secs: i64,
}

impl Default for PeerRateLimitConfig {
    fn default() -> Self {
        Self {
            max_capsules_per_hour: 100,
            window_secs: 3600,
        }
    }
}

pub struct PeerRateLimiter {
    config: PeerRateLimitConfig,
    windows: Mutex<HashMap<String, VecDeque<i64>>>,
}

impl PeerRateLimiter {
    pub fn new(config: PeerRateLimitConfig) -> Self {
        Self {
            config,
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> &PeerRateLimitConfig {
        &self.config
    }

    pub fn check(&self, peer_id: &str) -> bool {
        self.check_at(peer_id, Utc::now().timestamp())
    }

    pub fn check_at(&self, peer_id: &str, timestamp: i64) -> bool {
        let mut windows = self.windows.lock().unwrap();
        let entry = windows.entry(peer_id.to_string()).or_default();
        while let Some(front) = entry.front().copied() {
            if timestamp - front < self.config.window_secs {
                break;
            }
            entry.pop_front();
        }

        if entry.len() >= self.config.max_capsules_per_hour {
            return false;
        }

        entry.push_back(timestamp);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_blocks_after_capacity() {
        let limiter = PeerRateLimiter::new(PeerRateLimitConfig {
            max_capsules_per_hour: 2,
            window_secs: 3600,
        });

        assert!(limiter.check_at("peer-a", 10));
        assert!(limiter.check_at("peer-a", 11));
        assert!(!limiter.check_at("peer-a", 12));
    }

    #[test]
    fn rate_limiter_allows_after_window_expires() {
        let limiter = PeerRateLimiter::new(PeerRateLimitConfig {
            max_capsules_per_hour: 1,
            window_secs: 5,
        });

        assert!(limiter.check_at("peer-a", 10));
        assert!(!limiter.check_at("peer-a", 11));
        assert!(limiter.check_at("peer-a", 16));
    }
}
