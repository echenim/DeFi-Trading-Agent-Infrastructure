use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker to protect against cascading failures from unreliable services (e.g. RPC).
#[derive(Clone)]
pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}

struct CircuitBreakerInner {
    failure_threshold: u32,
    reset_timeout: Duration,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    state: RwLock<CircuitState>,
    last_failure_time: RwLock<Option<Instant>>,
    total_trips: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            inner: Arc::new(CircuitBreakerInner {
                failure_threshold,
                reset_timeout,
                failure_count: AtomicU32::new(0),
                success_count: AtomicU32::new(0),
                state: RwLock::new(CircuitState::Closed),
                last_failure_time: RwLock::new(None),
                total_trips: AtomicU64::new(0),
            }),
        }
    }

    /// Check if a request is allowed through the circuit breaker.
    pub async fn allow_request(&self) -> bool {
        let state = *self.inner.state.read().await;
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                let last_failure = self.inner.last_failure_time.read().await;
                if let Some(t) = *last_failure {
                    if t.elapsed() >= self.inner.reset_timeout {
                        drop(last_failure);
                        // Transition to half-open
                        let mut state = self.inner.state.write().await;
                        *state = CircuitState::HalfOpen;
                        info!("circuit breaker transitioning to half-open");
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                // Allow one probe request
                true
            }
        }
    }

    /// Record a successful operation.
    pub async fn record_success(&self) {
        self.inner.success_count.fetch_add(1, Ordering::Relaxed);
        let state = *self.inner.state.read().await;
        if state == CircuitState::HalfOpen {
            // Recovery confirmed — close the circuit
            let mut state = self.inner.state.write().await;
            *state = CircuitState::Closed;
            self.inner.failure_count.store(0, Ordering::Relaxed);
            info!("circuit breaker closed after successful probe");
        }
    }

    /// Record a failed operation.
    pub async fn record_failure(&self) {
        let count = self.inner.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        *self.inner.last_failure_time.write().await = Some(Instant::now());

        let state = *self.inner.state.read().await;

        match state {
            CircuitState::HalfOpen => {
                // Probe failed — reopen
                let mut state = self.inner.state.write().await;
                *state = CircuitState::Open;
                self.inner.total_trips.fetch_add(1, Ordering::Relaxed);
                warn!("circuit breaker re-opened after failed probe");
            }
            CircuitState::Closed if count >= self.inner.failure_threshold => {
                let mut state = self.inner.state.write().await;
                *state = CircuitState::Open;
                self.inner.total_trips.fetch_add(1, Ordering::Relaxed);
                warn!(
                    failures = count,
                    threshold = self.inner.failure_threshold,
                    "circuit breaker opened"
                );
            }
            _ => {}
        }
    }

    pub async fn state(&self) -> CircuitState {
        *self.inner.state.read().await
    }

    pub fn failure_count(&self) -> u32 {
        self.inner.failure_count.load(Ordering::Relaxed)
    }

    pub fn total_trips(&self) -> u64 {
        self.inner.total_trips.load(Ordering::Relaxed)
    }

    /// Reset the circuit breaker to closed state.
    pub async fn reset(&self) {
        let mut state = self.inner.state.write().await;
        *state = CircuitState::Closed;
        self.inner.failure_count.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_starts_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(5));
        assert_eq!(cb.state().await, CircuitState::Closed);
        assert!(cb.allow_request().await);
    }

    #[tokio::test]
    async fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(5));

        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Closed);

        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);
        assert!(!cb.allow_request().await);
    }

    #[tokio::test]
    async fn test_half_open_after_timeout() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(50));

        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);

        tokio::time::sleep(Duration::from_millis(60)).await;

        assert!(cb.allow_request().await);
        assert_eq!(cb.state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_half_open_success_closes() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(10));

        cb.record_failure().await;
        cb.record_failure().await;

        tokio::time::sleep(Duration::from_millis(20)).await;
        cb.allow_request().await; // triggers half-open

        cb.record_success().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }

    #[tokio::test]
    async fn test_half_open_failure_reopens() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(10));

        cb.record_failure().await;
        cb.record_failure().await;

        tokio::time::sleep(Duration::from_millis(20)).await;
        cb.allow_request().await; // triggers half-open

        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);
        assert_eq!(cb.total_trips(), 2);
    }

    #[tokio::test]
    async fn test_reset() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(60));
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);

        cb.reset().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }
}
