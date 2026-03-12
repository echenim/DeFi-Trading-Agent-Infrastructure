use std::future::Future;
use std::time::Duration;
use tracing::{debug, warn};

/// Configuration for retry with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_multiplier: f64,
}

impl RetryPolicy {
    pub fn new(max_retries: u32, initial_backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            max_retries,
            initial_backoff,
            max_backoff,
            backoff_multiplier: 2.0,
        }
    }

    pub fn from_config(config: &common::config::RetryConfig) -> Self {
        Self::new(
            config.max_retries,
            Duration::from_millis(config.initial_backoff_ms),
            Duration::from_millis(config.max_backoff_ms),
        )
    }
}

/// Retry an async operation with exponential backoff.
///
/// The operation function receives the attempt number (0-based).
/// Returns `Ok(T)` on first success, or the last `Err(E)` after exhausting retries.
pub async fn retry_with_backoff<F, Fut, T, E>(
    policy: &RetryPolicy,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut backoff = policy.initial_backoff;
    let mut last_err = None;

    for attempt in 0..=policy.max_retries {
        match operation(attempt).await {
            Ok(val) => {
                if attempt > 0 {
                    debug!(attempt, "operation succeeded after retry");
                }
                return Ok(val);
            }
            Err(e) => {
                if attempt < policy.max_retries {
                    warn!(
                        attempt,
                        max_retries = policy.max_retries,
                        backoff_ms = backoff.as_millis() as u64,
                        error = %e,
                        "operation failed, retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = Duration::from_secs_f64(
                        (backoff.as_secs_f64() * policy.backoff_multiplier)
                            .min(policy.max_backoff.as_secs_f64()),
                    );
                } else {
                    warn!(attempt, error = %e, "operation failed, no retries left");
                }
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_succeeds_first_try() {
        let policy = RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(10));

        let result: Result<i32, String> =
            retry_with_backoff(&policy, |_| async { Ok(42) }).await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_succeeds_after_retries() {
        let policy = RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(10));
        let call_count = Arc::new(AtomicU32::new(0));

        let cc = call_count.clone();
        let result: Result<i32, String> = retry_with_backoff(&policy, move |_attempt| {
            let cc = cc.clone();
            async move {
                let count = cc.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err("not yet".to_string())
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_exhausts_retries() {
        let policy = RetryPolicy::new(2, Duration::from_millis(1), Duration::from_millis(10));

        let result: Result<i32, String> =
            retry_with_backoff(&policy, |_| async { Err("fail".to_string()) }).await;

        assert_eq!(result.unwrap_err(), "fail");
    }

    #[tokio::test]
    async fn test_attempt_number_passed() {
        let policy = RetryPolicy::new(2, Duration::from_millis(1), Duration::from_millis(10));
        let attempts = Arc::new(std::sync::Mutex::new(vec![]));

        let att = attempts.clone();
        let _: Result<i32, String> = retry_with_backoff(&policy, move |attempt| {
            let att = att.clone();
            async move {
                att.lock().unwrap().push(attempt);
                Err("fail".to_string())
            }
        })
        .await;

        assert_eq!(*attempts.lock().unwrap(), vec![0, 1, 2]);
    }
}
