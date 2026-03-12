use async_trait::async_trait;
use common::errors::ExecutionError;
use common::types::TxHash;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

/// Trait for broadcasting signed transactions to the network.
#[async_trait]
pub trait Broadcaster: Send + Sync {
    async fn send_transaction(&self, signed_tx: &[u8]) -> Result<TxHash, ExecutionError>;
}

/// Broadcaster that logs what would be sent but does not actually broadcast.
///
/// Returns a deterministic dummy hash derived from the transaction bytes.
pub struct DryRunBroadcaster {
    counter: AtomicU64,
}

impl DryRunBroadcaster {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
}

impl Default for DryRunBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Broadcaster for DryRunBroadcaster {
    async fn send_transaction(&self, signed_tx: &[u8]) -> Result<TxHash, ExecutionError> {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        info!(
            tx_len = signed_tx.len(),
            seq = n,
            "DRY RUN: would broadcast transaction"
        );

        // Build a deterministic hash from the counter
        let mut hash = [0u8; 32];
        let n_bytes = n.to_be_bytes();
        hash[24..32].copy_from_slice(&n_bytes);
        Ok(TxHash(hash))
    }
}

/// Mock broadcaster for testing — returns pre-configured results.
pub struct MockBroadcaster {
    /// If set, every call returns this error message.
    fail_with: Option<String>,
}

impl MockBroadcaster {
    /// Create a mock broadcaster that succeeds, returning a dummy hash.
    pub fn success() -> Self {
        Self { fail_with: None }
    }

    /// Create a mock broadcaster that always fails with the given message.
    pub fn failing(reason: &str) -> Self {
        Self {
            fail_with: Some(reason.to_string()),
        }
    }
}

#[async_trait]
impl Broadcaster for MockBroadcaster {
    async fn send_transaction(&self, _signed_tx: &[u8]) -> Result<TxHash, ExecutionError> {
        if let Some(ref reason) = self.fail_with {
            return Err(ExecutionError::BroadcastFailed(reason.clone()));
        }
        // Return a recognizable dummy hash
        let mut hash = [0u8; 32];
        hash[31] = 0x01;
        Ok(TxHash(hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dry_run_broadcaster_returns_hash() {
        let broadcaster = DryRunBroadcaster::new();
        let result = broadcaster.send_transaction(b"fake signed tx").await;
        assert!(result.is_ok());
        let hash = result.unwrap();
        // Should be a valid TxHash
        assert_eq!(hash.0[31], 0); // counter starts at 0
    }

    #[tokio::test]
    async fn test_dry_run_broadcaster_increments() {
        let broadcaster = DryRunBroadcaster::new();
        let h1 = broadcaster.send_transaction(b"tx1").await.unwrap();
        let h2 = broadcaster.send_transaction(b"tx2").await.unwrap();
        assert_ne!(h1, h2);
    }

    #[tokio::test]
    async fn test_mock_broadcaster_success() {
        let broadcaster = MockBroadcaster::success();
        let result = broadcaster.send_transaction(b"tx").await;
        assert!(result.is_ok());
        let hash = result.unwrap();
        assert_eq!(hash.0[31], 0x01);
    }

    #[tokio::test]
    async fn test_mock_broadcaster_failure() {
        let broadcaster = MockBroadcaster::failing("network down");
        let result = broadcaster.send_transaction(b"tx").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("network down"));
    }
}
