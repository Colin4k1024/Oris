//! Three-state circuit breaker for the worker execution chain.
//!
//! State machine:
//! ```text
//! Closed ──trip()──► Open ──probe_window elapsed──► HalfOpen ──record_success()──► Closed
//!   ▲                                                   │
//!   └───────────────record_success()────────────────────┘
//!                    (shortcut on immediate recovery)
//! ```

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Snapshot of the circuit breaker state.
#[derive(Clone, Debug, PartialEq)]
pub enum CircuitState {
    /// Normal operation — dispatches proceed.
    Closed,
    /// Fault detected — dispatches are rejected until the probe window elapses.
    Open { opened_at: Instant },
    /// Probe phase — one dispatch is allowed through to test recovery.
    HalfOpen,
}

/// Thread-safe circuit breaker (Closed → Open → HalfOpen → Closed).
///
/// - [`CircuitBreaker::trip`] moves the breaker to `Open`.
/// - After [`probe_window_secs`](CircuitBreaker::new) elapses, `is_open()` auto-transitions to `HalfOpen`.
/// - [`CircuitBreaker::record_success`] resets to `Closed` from any non-Closed state.
pub struct CircuitBreaker {
    state: Mutex<CircuitState>,
    probe_window: Duration,
}

impl CircuitBreaker {
    /// Create a new `Closed` circuit breaker.
    ///
    /// `probe_window_secs` controls how long the breaker stays `Open` before
    /// transitioning to `HalfOpen` to allow a probe dispatch.
    pub fn new(probe_window_secs: u64) -> Self {
        Self {
            state: Mutex::new(CircuitState::Closed),
            probe_window: Duration::from_secs(probe_window_secs),
        }
    }

    /// Trip the breaker — moves it to `Open` regardless of current state.
    pub fn trip(&self) {
        let mut s = self.state.lock().expect("circuit breaker lock poisoned");
        *s = CircuitState::Open {
            opened_at: Instant::now(),
        };
    }

    /// Record a successful operation.
    ///
    /// If the breaker is `HalfOpen` or `Open`, resets it to `Closed`.
    pub fn record_success(&self) {
        let mut s = self.state.lock().expect("circuit breaker lock poisoned");
        match *s {
            CircuitState::HalfOpen | CircuitState::Open { .. } => {
                *s = CircuitState::Closed;
            }
            CircuitState::Closed => {}
        }
    }

    /// Returns `true` if the breaker is currently rejecting dispatches.
    ///
    /// When the probe window has elapsed for an `Open` breaker, this method
    /// automatically transitions to `HalfOpen` and returns `false`, allowing
    /// one probe dispatch through.
    pub fn is_open(&self) -> bool {
        let mut s = self.state.lock().expect("circuit breaker lock poisoned");
        match *s {
            CircuitState::Open { opened_at } => {
                if opened_at.elapsed() >= self.probe_window {
                    *s = CircuitState::HalfOpen;
                    false
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    /// Returns a snapshot of the current circuit state (without transitioning).
    pub fn current_state(&self) -> CircuitState {
        self.state
            .lock()
            .expect("circuit breaker lock poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn starts_closed() {
        let cb = CircuitBreaker::new(30);
        assert_eq!(cb.current_state(), CircuitState::Closed);
        assert!(!cb.is_open());
    }

    #[test]
    fn trip_opens_breaker() {
        let cb = CircuitBreaker::new(30);
        cb.trip();
        assert!(cb.is_open());
        match cb.current_state() {
            CircuitState::Open { .. } => {}
            other => panic!("expected Open, got {:?}", other),
        }
    }

    #[test]
    fn record_success_closes_open_breaker() {
        let cb = CircuitBreaker::new(30);
        cb.trip();
        assert!(cb.is_open());
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
        assert!(!cb.is_open());
    }

    #[test]
    fn record_success_closes_half_open_breaker() {
        // Use a very short probe window so it transitions to HalfOpen quickly.
        let cb = CircuitBreaker::new(0);
        cb.trip();
        thread::sleep(Duration::from_millis(5));
        // is_open() will auto-transition to HalfOpen
        assert!(!cb.is_open());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[test]
    fn open_transitions_to_half_open_after_probe_window() {
        let cb = CircuitBreaker::new(0); // 0-second window for tests
        cb.trip();
        thread::sleep(Duration::from_millis(5));
        // is_open() transitions to HalfOpen and returns false
        assert!(
            !cb.is_open(),
            "should not be open after probe window elapsed"
        );
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);
    }

    #[test]
    fn record_success_noop_when_already_closed() {
        let cb = CircuitBreaker::new(30);
        cb.record_success(); // should not panic
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }
}
