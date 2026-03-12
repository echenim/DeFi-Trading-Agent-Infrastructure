use async_trait::async_trait;
use common::errors::RpcError;
use common::types::TxHash;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info};

/// Raw pending transaction data from the mempool.
#[derive(Debug, Clone)]
pub struct RawTransaction {
    pub hash: TxHash,
    pub from: [u8; 20],
    pub to: Option<[u8; 20]>,
    pub value: u128,
    pub input: Vec<u8>,
    pub gas_price: u128,
}

/// Trait abstracting Ethereum RPC interactions so we can mock them in tests.
#[async_trait]
pub trait RpcClientTrait: Send + Sync {
    /// Subscribe to pending transaction hashes from the mempool.
    /// Returns a receiver that yields raw transaction data.
    async fn subscribe_pending_txs(
        &self,
    ) -> Result<mpsc::Receiver<Result<RawTransaction, RpcError>>, RpcError>;

    /// Fetch full transaction details by hash.
    async fn get_tx(&self, hash: &TxHash) -> Result<RawTransaction, RpcError>;
}

/// Production RPC client wrapping a WebSocket connection.
pub struct RpcClient {
    pub ws_url: String,
    pub http_url: String,
}

impl RpcClient {
    pub fn new(ws_url: String, http_url: String) -> Self {
        Self { ws_url, http_url }
    }
}

#[async_trait]
impl RpcClientTrait for RpcClient {
    async fn subscribe_pending_txs(
        &self,
    ) -> Result<mpsc::Receiver<Result<RawTransaction, RpcError>>, RpcError> {
        // Real WebSocket subscription would go here.
        // For now, return an error indicating no real connection.
        Err(RpcError::ConnectionFailed(format!(
            "real WebSocket connection to {} not implemented",
            self.ws_url
        )))
    }

    async fn get_tx(&self, hash: &TxHash) -> Result<RawTransaction, RpcError> {
        // Real HTTP/WS fetch would go here.
        Err(RpcError::ConnectionFailed(format!(
            "real get_tx for {hash} not implemented"
        )))
    }
}

/// Mock RPC client for testing. Yields pre-configured transactions.
pub struct MockRpcClient {
    transactions: Arc<Mutex<Vec<RawTransaction>>>,
}

impl MockRpcClient {
    pub fn new(transactions: Vec<RawTransaction>) -> Self {
        Self {
            transactions: Arc::new(Mutex::new(transactions)),
        }
    }
}

#[async_trait]
impl RpcClientTrait for MockRpcClient {
    async fn subscribe_pending_txs(
        &self,
    ) -> Result<mpsc::Receiver<Result<RawTransaction, RpcError>>, RpcError> {
        let (tx, rx) = mpsc::channel(256);
        let txs = self.transactions.lock().await.clone();
        info!(count = txs.len(), "mock: sending pre-configured transactions");

        tokio::spawn(async move {
            for raw_tx in txs {
                debug!(hash = %raw_tx.hash, "mock: yielding transaction");
                if tx.send(Ok(raw_tx)).await.is_err() {
                    break;
                }
            }
            // Channel closes when tx is dropped, signaling end of stream.
        });

        Ok(rx)
    }

    async fn get_tx(&self, hash: &TxHash) -> Result<RawTransaction, RpcError> {
        let txs = self.transactions.lock().await;
        txs.iter()
            .find(|t| t.hash == *hash)
            .cloned()
            .ok_or_else(|| RpcError::ProviderError(format!("tx {hash} not found in mock")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx_hash(n: u8) -> TxHash {
        let mut bytes = [0u8; 32];
        bytes[31] = n;
        TxHash(bytes)
    }

    fn make_raw_tx(n: u8, input: Vec<u8>) -> RawTransaction {
        RawTransaction {
            hash: make_tx_hash(n),
            from: [n; 20],
            to: Some([0xAA; 20]),
            value: 0,
            input,
            gas_price: 20_000_000_000,
        }
    }

    #[tokio::test]
    async fn test_mock_subscribe_yields_all_transactions() {
        let txs = vec![
            make_raw_tx(1, vec![0x38, 0xed, 0x17, 0x38]),
            make_raw_tx(2, vec![0x88, 0x03, 0xdb, 0xee]),
        ];

        let client = MockRpcClient::new(txs);
        let mut rx = client.subscribe_pending_txs().await.unwrap();

        let first = rx.recv().await.unwrap().unwrap();
        assert_eq!(first.hash, make_tx_hash(1));

        let second = rx.recv().await.unwrap().unwrap();
        assert_eq!(second.hash, make_tx_hash(2));

        // Stream ends
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn test_mock_get_tx_found() {
        let txs = vec![make_raw_tx(1, vec![])];
        let client = MockRpcClient::new(txs);

        let result = client.get_tx(&make_tx_hash(1)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_get_tx_not_found() {
        let client = MockRpcClient::new(vec![]);
        let result = client.get_tx(&make_tx_hash(99)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_real_client_returns_error() {
        let client = RpcClient::new("ws://localhost:9999".into(), "http://localhost:9999".into());
        let result = client.subscribe_pending_txs().await;
        assert!(result.is_err());
    }
}
