use common::messages::{Envelope, Message};
use messaging::bus::MessageBus;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, info_span, warn, Instrument};

use crate::strategy::Strategy;

/// The strategy agent holds a set of strategies and routes market signals to them.
///
/// When a strategy produces a `TradeIntent`, the agent wraps it in an envelope
/// and publishes it to the message bus as `Message::Intent`.
pub struct StrategyAgent {
    strategies: Vec<Box<dyn Strategy>>,
    bus: MessageBus,
    cancel: CancellationToken,
}

impl StrategyAgent {
    pub fn new(
        strategies: Vec<Box<dyn Strategy>>,
        bus: MessageBus,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            strategies,
            bus,
            cancel,
        }
    }

    /// Start the agent loop: subscribe to the bus, filter for signals,
    /// evaluate each strategy, and publish resulting intents.
    pub async fn start(&self) {
        let mut subscriber = self.bus.subscribe();
        let _span = info_span!("strategy_agent");

        info!(
            strategy_count = self.strategies.len(),
            "strategy agent starting"
        );

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    info!("strategy agent shutting down");
                    break;
                }
                result = subscriber.recv() => {
                    match result {
                        Ok(Message::Signal(envelope)) => {
                            let signal = &envelope.payload;
                            debug!(signal_type = ?signal.signal_type, "received market signal");

                            for strategy in &self.strategies {
                                let strategy_span = info_span!("evaluate", strategy = strategy.name());
                                let intent = strategy
                                    .evaluate(signal)
                                    .instrument(strategy_span)
                                    .await;

                                if let Some(trade_intent) = intent {
                                    debug!(
                                        strategy = strategy.name(),
                                        profit_bps = trade_intent.expected_profit_bps,
                                        "strategy produced trade intent"
                                    );
                                    let msg = Message::Intent(Envelope::new(trade_intent));
                                    if let Err(e) = self.bus.publish(msg) {
                                        warn!(error = %e, "failed to publish trade intent");
                                    }
                                }
                            }
                        }
                        Ok(_) => {
                            // Ignore non-signal messages
                        }
                        Err(e) => {
                            warn!(error = %e, "bus receive error");
                            break;
                        }
                    }
                }
            }
        }

        info!("strategy agent stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::arbitrage::ArbitrageStrategy;
    use common::messages::{MarketSignal, SignalType};
    use common::types::Dex;

    #[tokio::test]
    async fn test_strategy_agent_publishes_intents() {
        let bus = MessageBus::with_default_capacity();
        let cancel = CancellationToken::new();

        let strategy = ArbitrageStrategy::new(50.0, 10.0);
        let agent = StrategyAgent::new(
            vec![Box::new(strategy)],
            bus.clone(),
            cancel.clone(),
        );

        // Subscribe before the agent starts so we can read intents
        let mut intent_sub = bus.subscribe();

        // Spawn the agent
        let agent_handle = tokio::spawn(async move {
            agent.start().await;
        });

        // Give the agent a moment to subscribe
        tokio::task::yield_now().await;

        // Publish a profitable signal
        let signal = MarketSignal {
            signal_type: SignalType::PriceDivergence {
                pair_a_dex: Dex::UniswapV2,
                pair_b_dex: Dex::SushiSwap,
                spread_bps: 100.0,
            },
            quotes: vec![],
            source_tx: None,
        };
        bus.publish(Message::Signal(Envelope::new(signal))).unwrap();

        // Read messages until we get an Intent
        let mut found_intent = false;
        for _ in 0..10 {
            tokio::task::yield_now().await;
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                intent_sub.recv(),
            )
            .await
            {
                Ok(Ok(Message::Intent(env))) => {
                    assert_eq!(env.payload.strategy_name, "arbitrage");
                    assert_eq!(env.payload.expected_profit_bps, 100.0);
                    found_intent = true;
                    break;
                }
                Ok(Ok(_)) => continue, // skip the signal echo
                Ok(Err(_)) => break,
                Err(_) => break, // timeout
            }
        }

        cancel.cancel();
        let _ = agent_handle.await;
        assert!(found_intent, "expected to receive a trade intent");
    }
}
