use common::config::RiskConfig;
use common::messages::{Envelope, Message, ValidatedTrade};
use messaging::bus::MessageBus;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::rules;
use crate::state::ExposureTracker;

/// The risk agent validates trade intents against configured risk limits.
///
/// It subscribes to the message bus, filters for `Message::Intent` messages,
/// runs all risk checks, and publishes `Message::Validated` for approved trades.
pub struct RiskAgent {
    tracker: ExposureTracker,
    config: RiskConfig,
    bus: MessageBus,
    cancel: CancellationToken,
}

impl RiskAgent {
    pub fn new(
        config: RiskConfig,
        bus: MessageBus,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            tracker: ExposureTracker::new(),
            config,
            bus,
            cancel,
        }
    }

    /// Access the exposure tracker (useful for tests and monitoring).
    pub fn tracker(&self) -> &ExposureTracker {
        &self.tracker
    }

    /// Start the agent loop: subscribe to the bus, filter for intents,
    /// run all risk checks, and publish validated trades.
    pub async fn start(&self) {
        let mut subscriber = self.bus.subscribe();

        info!(
            max_position_eth = self.config.max_position_size_eth,
            max_slippage_bps = self.config.max_slippage_bps,
            max_gas_gwei = self.config.max_gas_price_gwei,
            daily_loss_limit = self.config.daily_loss_limit_eth,
            "risk agent starting"
        );

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    info!("risk agent shutting down");
                    break;
                }
                result = subscriber.recv() => {
                    match result {
                        Ok(Message::Intent(envelope)) => {
                            let intent = &envelope.payload;
                            debug!(
                                strategy = %intent.strategy_name,
                                amount_wei = %intent.amount_in_wei,
                                "received trade intent"
                            );

                            if let Err(e) = self.validate(intent) {
                                warn!(
                                    strategy = %intent.strategy_name,
                                    error = %e,
                                    "trade intent rejected"
                                );
                                continue;
                            }

                            // All checks passed — compute a simple risk score and publish.
                            let risk_score = self.compute_risk_score(intent);

                            // Track the new exposure
                            let amount_eth = wei_to_eth(&intent.amount_in_wei);
                            self.tracker.add_exposure(amount_eth);

                            let validated = ValidatedTrade {
                                intent: intent.clone(),
                                risk_score,
                                approved_at_ms: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64,
                            };

                            let msg = Message::Validated(Envelope::new(validated));
                            if let Err(e) = self.bus.publish(msg) {
                                warn!(error = %e, "failed to publish validated trade");
                            } else {
                                debug!(
                                    strategy = %intent.strategy_name,
                                    risk_score,
                                    "trade validated and published"
                                );
                            }
                        }
                        Ok(_) => {
                            // Ignore non-intent messages
                        }
                        Err(e) => {
                            warn!(error = %e, "bus receive error");
                            break;
                        }
                    }
                }
            }
        }

        info!("risk agent stopped");
    }

    /// Run all risk validation rules against an intent.
    fn validate(&self, intent: &common::messages::TradeIntent) -> Result<(), common::errors::RiskError> {
        rules::check_sanity(intent)?;
        rules::check_exposure_limit(intent, &self.tracker, self.config.max_position_size_eth)?;
        rules::check_slippage(intent, self.config.max_slippage_bps)?;
        rules::check_gas_price(intent, self.config.max_gas_price_gwei)?;
        rules::check_daily_loss_limit(&self.tracker, self.config.daily_loss_limit_eth)?;
        Ok(())
    }

    /// Compute a simple risk score in [0.0, 1.0] where lower is better (less risky).
    fn compute_risk_score(&self, intent: &common::messages::TradeIntent) -> f64 {
        let amount_eth = wei_to_eth(&intent.amount_in_wei);
        let exposure_ratio = (self.tracker.current_exposure() + amount_eth)
            / self.config.max_position_size_eth;

        let gas_ratio = intent
            .max_gas_price_gwei
            .map(|g| g / self.config.max_gas_price_gwei)
            .unwrap_or(0.0);

        let loss_ratio = self.tracker.daily_loss() / self.config.daily_loss_limit_eth;

        // Weighted average, clamped to [0, 1]
        let score = 0.4 * exposure_ratio + 0.3 * gas_ratio + 0.3 * loss_ratio;
        score.clamp(0.0, 1.0)
    }
}

fn wei_to_eth(wei_str: &str) -> f64 {
    let wei: f64 = wei_str.parse().unwrap_or(0.0);
    wei / 1e18
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::config::RiskConfig;
    use common::messages::{Envelope, TradeIntent};
    use common::types::{Address, Dex, TradeSide};

    fn test_config() -> RiskConfig {
        RiskConfig {
            max_position_size_eth: 10.0,
            max_slippage_bps: 200,
            max_gas_price_gwei: 100.0,
            daily_loss_limit_eth: 2.0,
        }
    }

    fn make_good_intent() -> TradeIntent {
        TradeIntent {
            strategy_name: "test_arb".to_string(),
            token_in: Address::zero(),
            token_out: Address([1u8; 20]),
            amount_in_wei: "1000000000000000000".to_string(), // 1 ETH
            min_amount_out_wei: "990000000000000000".to_string(),
            dex: Dex::UniswapV2,
            side: TradeSide::Buy,
            expected_profit_bps: 50.0,
            deadline_secs: 120,
            max_gas_price_gwei: Some(50.0),
        }
    }

    fn make_bad_intent() -> TradeIntent {
        TradeIntent {
            strategy_name: "bad_strategy".to_string(),
            token_in: Address::zero(),
            token_out: Address::zero(), // same token — fails sanity
            amount_in_wei: "0".to_string(), // zero amount — fails sanity
            min_amount_out_wei: "0".to_string(),
            dex: Dex::UniswapV2,
            side: TradeSide::Buy,
            expected_profit_bps: 0.0,
            deadline_secs: 0, // zero deadline — fails sanity
            max_gas_price_gwei: Some(500.0), // too high gas
        }
    }

    #[tokio::test]
    async fn test_risk_agent_validates_good_intent() {
        let bus = MessageBus::with_default_capacity();
        let cancel = CancellationToken::new();
        let agent = RiskAgent::new(test_config(), bus.clone(), cancel.clone());

        // Subscribe to catch validated messages
        let mut sub = bus.subscribe();

        // Spawn the agent
        let agent_handle = tokio::spawn(async move {
            agent.start().await;
        });

        // Let the agent subscribe
        tokio::task::yield_now().await;

        // Publish a good intent
        let intent = make_good_intent();
        bus.publish(Message::Intent(Envelope::new(intent)))
            .unwrap();

        // Look for the Validated message
        let mut found_validated = false;
        for _ in 0..10 {
            tokio::task::yield_now().await;
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                sub.recv(),
            )
            .await
            {
                Ok(Ok(Message::Validated(env))) => {
                    assert_eq!(env.payload.intent.strategy_name, "test_arb");
                    assert!(env.payload.risk_score >= 0.0);
                    assert!(env.payload.risk_score <= 1.0);
                    found_validated = true;
                    break;
                }
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }

        cancel.cancel();
        let _ = agent_handle.await;
        assert!(found_validated, "expected a validated trade to be published");
    }

    #[tokio::test]
    async fn test_risk_agent_rejects_bad_intent() {
        let bus = MessageBus::with_default_capacity();
        let cancel = CancellationToken::new();
        let agent = RiskAgent::new(test_config(), bus.clone(), cancel.clone());

        let mut sub = bus.subscribe();

        let agent_handle = tokio::spawn(async move {
            agent.start().await;
        });

        tokio::task::yield_now().await;

        // Publish a bad intent
        let intent = make_bad_intent();
        bus.publish(Message::Intent(Envelope::new(intent)))
            .unwrap();

        // We should NOT receive a Validated message; only the Intent echo
        let mut found_validated = false;
        for _ in 0..5 {
            tokio::task::yield_now().await;
            match tokio::time::timeout(
                std::time::Duration::from_millis(100),
                sub.recv(),
            )
            .await
            {
                Ok(Ok(Message::Validated(_))) => {
                    found_validated = true;
                    break;
                }
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }

        cancel.cancel();
        let _ = agent_handle.await;
        assert!(
            !found_validated,
            "bad intent should have been rejected, not validated"
        );
    }
}
