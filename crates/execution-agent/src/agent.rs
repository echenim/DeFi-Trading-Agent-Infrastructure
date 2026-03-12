use std::sync::Arc;

use common::config::ExecutionConfig;
use common::messages::{Envelope, ExecutionOutcome, ExecutionResult, Message};
use messaging::bus::MessageBus;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, info_span, warn, Instrument};

use crate::broadcast::{Broadcaster, DryRunBroadcaster};
use crate::gas::GasEstimator;
use crate::nonce::NonceManager;
use crate::signer::Signer;
use crate::tx_builder::TransactionBuilder;

/// The execution agent receives validated trades, builds and signs transactions,
/// broadcasts them, and publishes the result back to the bus.
pub struct ExecutionAgent {
    signer: Arc<dyn Signer>,
    nonce_manager: Arc<NonceManager>,
    tx_builder: TransactionBuilder,
    broadcaster: Arc<dyn Broadcaster>,
    config: ExecutionConfig,
    bus: MessageBus,
    cancel: CancellationToken,
}

impl ExecutionAgent {
    /// Create a new execution agent.
    ///
    /// If `config.dry_run` is true, the provided broadcaster is ignored and a
    /// `DryRunBroadcaster` is used instead.
    pub fn new(
        signer: Arc<dyn Signer>,
        nonce_manager: Arc<NonceManager>,
        tx_builder: TransactionBuilder,
        broadcaster: Arc<dyn Broadcaster>,
        config: ExecutionConfig,
        bus: MessageBus,
        cancel: CancellationToken,
    ) -> Self {
        let broadcaster: Arc<dyn Broadcaster> = if config.dry_run {
            Arc::new(DryRunBroadcaster::new())
        } else {
            broadcaster
        };

        Self {
            signer,
            nonce_manager,
            tx_builder,
            broadcaster,
            config,
            bus,
            cancel,
        }
    }

    /// Start the agent loop: subscribe to the bus, filter for `Message::Validated`,
    /// and process each validated trade.
    pub async fn start(&self) {
        let mut subscriber = self.bus.subscribe();
        let span = info_span!("execution_agent");

        info!(
            dry_run = self.config.dry_run,
            signer_address = %self.signer.address(),
            "execution agent starting"
        );

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    info!("execution agent shutting down");
                    break;
                }
                result = subscriber.recv() => {
                    match result {
                        Ok(Message::Validated(envelope)) => {
                            let trade = envelope.payload.clone();
                            debug!(
                                strategy = %trade.intent.strategy_name,
                                risk_score = trade.risk_score,
                                "received validated trade"
                            );

                            let outcome = self.execute_trade(&trade)
                                .instrument(span.clone())
                                .await;

                            let exec_result = ExecutionResult {
                                trade,
                                outcome,
                            };

                            let msg = Message::Executed(Envelope::new(exec_result));
                            if let Err(e) = self.bus.publish(msg) {
                                warn!(error = %e, "failed to publish execution result");
                            }
                        }
                        Ok(_) => {
                            // Ignore non-validated messages
                        }
                        Err(e) => {
                            warn!(error = %e, "bus receive error");
                            break;
                        }
                    }
                }
            }
        }

        info!("execution agent stopped");
    }

    /// Execute a single validated trade: build tx, sign, broadcast.
    async fn execute_trade(
        &self,
        trade: &common::messages::ValidatedTrade,
    ) -> ExecutionOutcome {
        // 1. Get nonce
        let nonce = self.nonce_manager.next();

        // 2. Estimate gas
        let base_gas: u64 = 200_000; // default estimate for a swap
        let gas_limit = GasEstimator::estimate_gas(base_gas, self.config.gas_price_multiplier);

        // 3. Determine gas price
        let gas_price_gwei = trade
            .intent
            .max_gas_price_gwei
            .unwrap_or(30.0);

        // 4. Build the transaction
        let raw_tx = self.tx_builder.build(trade, nonce, gas_limit, gas_price_gwei);

        // 5. Encode and sign
        let tx_bytes = raw_tx.encode();
        let signed_tx = match self.signer.sign(&tx_bytes).await {
            Ok(s) => s,
            Err(e) => {
                return ExecutionOutcome::Failed {
                    reason: format!("signing failed: {e}"),
                };
            }
        };

        // 6. Broadcast
        if self.config.dry_run {
            // Still call broadcaster (which is DryRunBroadcaster) for logging
            let _ = self.broadcaster.send_transaction(&signed_tx).await;
            return ExecutionOutcome::DryRun {
                would_send: format!(
                    "nonce={} gas_limit={} gas_price={:.2}gwei to={} value={}",
                    nonce, gas_limit, gas_price_gwei, raw_tx.to, raw_tx.value
                ),
            };
        }

        match self.broadcaster.send_transaction(&signed_tx).await {
            Ok(tx_hash) => {
                info!(%tx_hash, nonce, "transaction broadcast successfully");
                ExecutionOutcome::Success {
                    tx_hash,
                    gas_used: gas_limit,
                    effective_gas_price_gwei: gas_price_gwei,
                }
            }
            Err(e) => ExecutionOutcome::Failed {
                reason: format!("broadcast failed: {e}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::MockBroadcaster;
    use crate::signer::MockSigner;
    use common::messages::{TradeIntent, ValidatedTrade};
    use common::types::{Address, Dex, TradeSide};

    fn sample_validated_trade() -> ValidatedTrade {
        ValidatedTrade {
            intent: TradeIntent {
                strategy_name: "arb_v1".to_string(),
                token_in: Address::zero(),
                token_out: Address([0xBB; 20]),
                amount_in_wei: "1000000000000000000".to_string(),
                min_amount_out_wei: "990000000000000000".to_string(),
                dex: Dex::UniswapV2,
                side: TradeSide::Buy,
                expected_profit_bps: 75.0,
                deadline_secs: 120,
                max_gas_price_gwei: Some(50.0),
            },
            risk_score: 0.25,
            approved_at_ms: 1700000000000,
        }
    }

    fn make_config(dry_run: bool) -> ExecutionConfig {
        ExecutionConfig {
            gas_price_multiplier: 1.2,
            dry_run,
            confirmation_blocks: 1,
            tx_timeout_secs: 30,
        }
    }

    #[tokio::test]
    async fn test_agent_dry_run_processes_validated_trade() {
        let bus = MessageBus::with_default_capacity();
        let cancel = CancellationToken::new();

        let signer = Arc::new(MockSigner::new(Address::zero()));
        let nonce_mgr = Arc::new(NonceManager::new(0));
        let tx_builder = TransactionBuilder::new(1);
        let broadcaster = Arc::new(MockBroadcaster::success());
        let config = make_config(true); // dry run

        let agent = ExecutionAgent::new(
            signer,
            nonce_mgr,
            tx_builder,
            broadcaster,
            config,
            bus.clone(),
            cancel.clone(),
        );

        // Subscribe to catch the Executed message
        let mut result_sub = bus.subscribe();

        // Spawn the agent
        let agent_handle = tokio::spawn(async move {
            agent.start().await;
        });

        // Give the agent time to subscribe
        tokio::task::yield_now().await;

        // Publish a validated trade
        let trade = sample_validated_trade();
        bus.publish(Message::Validated(Envelope::new(trade)))
            .unwrap();

        // Wait for the execution result
        let mut found_result = false;
        for _ in 0..20 {
            tokio::task::yield_now().await;
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                result_sub.recv(),
            )
            .await
            {
                Ok(Ok(Message::Executed(env))) => {
                    match &env.payload.outcome {
                        ExecutionOutcome::DryRun { would_send } => {
                            assert!(would_send.contains("nonce=0"));
                            found_result = true;
                            break;
                        }
                        other => panic!("expected DryRun outcome, got: {other:?}"),
                    }
                }
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }

        cancel.cancel();
        let _ = agent_handle.await;
        assert!(found_result, "expected to receive an execution result");
    }

    #[tokio::test]
    async fn test_agent_live_mode_broadcasts() {
        let bus = MessageBus::with_default_capacity();
        let cancel = CancellationToken::new();

        let signer = Arc::new(MockSigner::new(Address::zero()));
        let nonce_mgr = Arc::new(NonceManager::new(5));
        let tx_builder = TransactionBuilder::new(1);
        let broadcaster = Arc::new(MockBroadcaster::success());
        let config = make_config(false); // live mode

        let agent = ExecutionAgent::new(
            signer,
            nonce_mgr,
            tx_builder,
            broadcaster,
            config,
            bus.clone(),
            cancel.clone(),
        );

        let mut result_sub = bus.subscribe();

        let agent_handle = tokio::spawn(async move {
            agent.start().await;
        });

        tokio::task::yield_now().await;

        let trade = sample_validated_trade();
        bus.publish(Message::Validated(Envelope::new(trade)))
            .unwrap();

        let mut found_result = false;
        for _ in 0..20 {
            tokio::task::yield_now().await;
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                result_sub.recv(),
            )
            .await
            {
                Ok(Ok(Message::Executed(env))) => {
                    match &env.payload.outcome {
                        ExecutionOutcome::Success {
                            tx_hash,
                            gas_used,
                            effective_gas_price_gwei,
                        } => {
                            assert_eq!(tx_hash.0[31], 0x01); // MockBroadcaster hash
                            assert!(*gas_used > 0);
                            assert!(*effective_gas_price_gwei > 0.0);
                            found_result = true;
                            break;
                        }
                        other => panic!("expected Success outcome, got: {other:?}"),
                    }
                }
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }

        cancel.cancel();
        let _ = agent_handle.await;
        assert!(found_result, "expected to receive a success execution result");
    }

    #[tokio::test]
    async fn test_agent_handles_broadcast_failure() {
        let bus = MessageBus::with_default_capacity();
        let cancel = CancellationToken::new();

        let signer = Arc::new(MockSigner::new(Address::zero()));
        let nonce_mgr = Arc::new(NonceManager::new(0));
        let tx_builder = TransactionBuilder::new(1);
        let broadcaster = Arc::new(MockBroadcaster::failing("rpc unreachable"));
        let config = make_config(false);

        let agent = ExecutionAgent::new(
            signer,
            nonce_mgr,
            tx_builder,
            broadcaster,
            config,
            bus.clone(),
            cancel.clone(),
        );

        let mut result_sub = bus.subscribe();

        let agent_handle = tokio::spawn(async move {
            agent.start().await;
        });

        tokio::task::yield_now().await;

        let trade = sample_validated_trade();
        bus.publish(Message::Validated(Envelope::new(trade)))
            .unwrap();

        let mut found_result = false;
        for _ in 0..20 {
            tokio::task::yield_now().await;
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                result_sub.recv(),
            )
            .await
            {
                Ok(Ok(Message::Executed(env))) => {
                    match &env.payload.outcome {
                        ExecutionOutcome::Failed { reason } => {
                            assert!(reason.contains("rpc unreachable"));
                            found_result = true;
                            break;
                        }
                        other => panic!("expected Failed outcome, got: {other:?}"),
                    }
                }
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }

        cancel.cancel();
        let _ = agent_handle.await;
        assert!(found_result, "expected to receive a failed execution result");
    }

    #[tokio::test]
    async fn test_agent_graceful_shutdown() {
        let bus = MessageBus::with_default_capacity();
        let cancel = CancellationToken::new();

        let signer = Arc::new(MockSigner::new(Address::zero()));
        let nonce_mgr = Arc::new(NonceManager::new(0));
        let tx_builder = TransactionBuilder::new(1);
        let broadcaster = Arc::new(MockBroadcaster::success());
        let config = make_config(true);

        let agent = ExecutionAgent::new(
            signer,
            nonce_mgr,
            tx_builder,
            broadcaster,
            config,
            bus.clone(),
            cancel.clone(),
        );

        let agent_handle = tokio::spawn(async move {
            agent.start().await;
        });

        tokio::task::yield_now().await;
        cancel.cancel();

        // Agent should stop within a reasonable time
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            agent_handle,
        )
        .await;

        assert!(result.is_ok(), "agent should shut down promptly");
    }
}
