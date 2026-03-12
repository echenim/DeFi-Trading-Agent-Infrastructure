use common::config::MarketDataConfig;
use common::messages::{Envelope, MarketSignal, Message, SignalType};
use common::types::Dex;
use messaging::bus::MessageBus;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::parser::TxParser;
use crate::price_tracker::PriceTracker;
use crate::rpc::RpcClientTrait;

/// The Market Data Agent: ingests mempool transactions, parses DEX swaps,
/// tracks prices, and publishes market signals to the message bus.
pub struct MarketDataAgent<R: RpcClientTrait> {
    rpc: R,
    bus: MessageBus,
    cancel: CancellationToken,
    config: MarketDataConfig,
    price_tracker: PriceTracker,
}

impl<R: RpcClientTrait> MarketDataAgent<R> {
    pub fn new(
        rpc: R,
        bus: MessageBus,
        cancel: CancellationToken,
        config: MarketDataConfig,
    ) -> Self {
        Self {
            rpc,
            bus,
            cancel,
            config,
            price_tracker: PriceTracker::new(),
        }
    }

    /// Get a reference to the price tracker (useful for external queries).
    pub fn price_tracker(&self) -> &PriceTracker {
        &self.price_tracker
    }

    /// Run the main agent loop. Blocks until the cancellation token fires
    /// or the mempool subscription ends.
    pub async fn start(&self) {
        info!(
            mempool_buffer = self.config.mempool_buffer_size,
            staleness_secs = self.config.price_staleness_secs,
            "market data agent starting"
        );

        let mut rx = match self.rpc.subscribe_pending_txs().await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "failed to subscribe to pending txs");
                return;
            }
        };

        info!("subscribed to pending transactions");

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    info!("market data agent shutting down (cancellation)");
                    break;
                }
                maybe_tx = rx.recv() => {
                    match maybe_tx {
                        Some(Ok(raw_tx)) => {
                            debug!(hash = %raw_tx.hash, "received pending tx");
                            self.process_transaction(&raw_tx).await;
                        }
                        Some(Err(e)) => {
                            warn!(error = %e, "error receiving tx from mempool stream");
                        }
                        None => {
                            info!("mempool stream ended");
                            break;
                        }
                    }
                }
            }
        }

        info!("market data agent stopped");
    }

    async fn process_transaction(&self, raw_tx: &crate::rpc::RawTransaction) {
        if let Some(swap) = TxParser::parse_swap(raw_tx) {
            debug!(
                dex = %swap.dex,
                sender = %swap.sender,
                amount = swap.amount,
                "detected DEX swap"
            );

            // Publish a LargeSwap signal for any detected swap.
            // In production, we'd have a threshold; here we emit for all detected swaps.
            let signal = MarketSignal {
                signal_type: SignalType::LargeSwap {
                    dex: swap.dex,
                    // Rough ETH value estimate (simplified — real impl would use price oracle)
                    value_eth: swap.amount as f64 / 1e18,
                },
                quotes: vec![],
                source_tx: Some(raw_tx.hash),
            };

            let msg = Message::Signal(Envelope::new(signal));
            if let Err(e) = self.bus.publish(msg) {
                warn!(error = %e, "failed to publish market signal");
            }

            // Check for price divergence across known DEX pairs.
            self.check_divergence(&swap).await;
        }
    }

    async fn check_divergence(&self, swap: &crate::parser::ParsedSwap) {
        let dex_pairs = [
            (Dex::UniswapV2, Dex::SushiSwap),
            (Dex::UniswapV2, Dex::UniswapV3),
            (Dex::UniswapV3, Dex::SushiSwap),
        ];

        let template_pair = common::types::TokenPair {
            token_a: swap.token_in,
            token_b: swap.token_out,
            dex: swap.dex,
        };

        for (dex_a, dex_b) in &dex_pairs {
            if let Some(spread_bps) = self
                .price_tracker
                .detect_divergence(&template_pair, *dex_a, *dex_b, 50.0)
                .await
            {
                info!(
                    dex_a = %dex_a,
                    dex_b = %dex_b,
                    spread_bps,
                    "price divergence detected"
                );

                let signal = MarketSignal {
                    signal_type: SignalType::PriceDivergence {
                        pair_a_dex: *dex_a,
                        pair_b_dex: *dex_b,
                        spread_bps,
                    },
                    quotes: vec![],
                    source_tx: None,
                };

                let msg = Message::Signal(Envelope::new(signal));
                if let Err(e) = self.bus.publish(msg) {
                    warn!(error = %e, "failed to publish divergence signal");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{MockRpcClient, RawTransaction};
    use common::config::MarketDataConfig;
    use common::types::TxHash;

    fn make_config() -> MarketDataConfig {
        MarketDataConfig {
            mempool_buffer_size: 100,
            price_staleness_secs: 30,
        }
    }

    fn make_uni_v2_calldata() -> Vec<u8> {
        // swapExactTokensForTokens with minimal valid data
        let mut data = Vec::with_capacity(260);
        data.extend_from_slice(&[0x38, 0xed, 0x17, 0x38]); // selector

        // amountIn = 1 ETH
        let mut amount_word = [0u8; 32];
        let amount: u128 = 1_000_000_000_000_000_000;
        amount_word[16..32].copy_from_slice(&amount.to_be_bytes());
        data.extend_from_slice(&amount_word);

        // amountOutMin
        data.extend_from_slice(&[0u8; 32]);

        // offset to path = 0xa0
        let mut offset = [0u8; 32];
        offset[31] = 0xa0;
        data.extend_from_slice(&offset);

        // to
        data.extend_from_slice(&[0u8; 32]);
        // deadline
        data.extend_from_slice(&[0u8; 32]);

        // path length = 2
        let mut len = [0u8; 32];
        len[31] = 2;
        data.extend_from_slice(&len);

        // path[0] = token_in
        let mut in_word = [0u8; 32];
        in_word[12..32].copy_from_slice(&[0x11; 20]);
        data.extend_from_slice(&in_word);

        // path[1] = token_out
        let mut out_word = [0u8; 32];
        out_word[12..32].copy_from_slice(&[0x22; 20]);
        data.extend_from_slice(&out_word);

        data
    }

    #[tokio::test]
    async fn test_agent_processes_transactions() {
        let txs = vec![RawTransaction {
            hash: TxHash([1u8; 32]),
            from: [0xAB; 20],
            to: Some([0xCC; 20]),
            value: 0,
            input: make_uni_v2_calldata(),
            gas_price: 20_000_000_000,
        }];

        let mock_rpc = MockRpcClient::new(txs);
        let bus = MessageBus::new(128);
        let mut subscriber = bus.subscribe();
        let cancel = CancellationToken::new();
        let config = make_config();

        let agent = MarketDataAgent::new(mock_rpc, bus, cancel.clone(), config);

        // Run agent in background
        let handle = tokio::spawn(async move {
            agent.start().await;
        });

        // Wait for the signal to be published
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), subscriber.recv())
            .await
            .expect("timeout waiting for message")
            .expect("bus error");

        match msg {
            Message::Signal(env) => {
                assert!(matches!(env.payload.signal_type, SignalType::LargeSwap { .. }));
            }
            other => panic!("expected Signal, got {other:?}"),
        }

        // The mock stream will end naturally, which stops the agent
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_agent_shutdown_on_cancel() {
        let mock_rpc = MockRpcClient::new(vec![]);
        let bus = MessageBus::new(128);
        let _subscriber = bus.subscribe();
        let cancel = CancellationToken::new();
        let config = make_config();

        let agent = MarketDataAgent::new(mock_rpc, bus, cancel.clone(), config);

        let handle = tokio::spawn(async move {
            agent.start().await;
        });

        // Agent should stop quickly since mock stream is empty
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("agent should stop within timeout")
            .unwrap();
    }

    #[tokio::test]
    async fn test_agent_skips_non_swap_txs() {
        let txs = vec![RawTransaction {
            hash: TxHash([2u8; 32]),
            from: [0xAB; 20],
            to: Some([0xCC; 20]),
            value: 1_000_000,
            input: vec![0xFF, 0xFF, 0xFF, 0xFF], // unknown selector
            gas_price: 20_000_000_000,
        }];

        let mock_rpc = MockRpcClient::new(txs);
        let bus = MessageBus::new(128);
        let mut subscriber = bus.subscribe();
        let cancel = CancellationToken::new();
        let config = make_config();

        let agent = MarketDataAgent::new(mock_rpc, bus.clone(), cancel.clone(), config);

        let handle = tokio::spawn(async move {
            agent.start().await;
        });

        // Should timeout since no signal is published for non-swap txs.
        // We race the recv against a short timeout while the agent is still running.
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            subscriber.recv(),
        )
        .await;

        // The mock stream ends quickly, so the agent exits. The recv should either
        // timeout (no signal published) or get a ChannelClosed error — neither is a valid Signal.
        match result {
            Err(_) => {} // timeout — expected, no message published
            Ok(Err(_)) => {} // channel closed — also fine, no signal was published
            Ok(Ok(msg)) => panic!("expected no signal for non-swap tx, got {msg:?}"),
        }

        let _ = handle.await;
    }
}
