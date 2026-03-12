use async_trait::async_trait;
use common::messages::{MarketSignal, SignalType, TradeIntent};
use common::types::{Address, TradeSide};
use tracing::debug;

use crate::strategy::Strategy;

/// Arbitrage strategy that detects price divergences between DEXes.
///
/// When the spread between two DEXes exceeds `min_profit_bps`, it generates
/// a `TradeIntent` to exploit the difference.
pub struct ArbitrageStrategy {
    min_profit_bps: f64,
    max_trade_size_eth: f64,
}

impl ArbitrageStrategy {
    pub fn new(min_profit_bps: f64, max_trade_size_eth: f64) -> Self {
        Self {
            min_profit_bps,
            max_trade_size_eth,
        }
    }
}

#[async_trait]
impl Strategy for ArbitrageStrategy {
    fn name(&self) -> &str {
        "arbitrage"
    }

    async fn evaluate(&self, signal: &MarketSignal) -> Option<TradeIntent> {
        match &signal.signal_type {
            SignalType::PriceDivergence {
                pair_a_dex,
                pair_b_dex: _,
                spread_bps,
            } => {
                if *spread_bps < self.min_profit_bps {
                    debug!(
                        spread_bps,
                        min_profit_bps = self.min_profit_bps,
                        "spread below threshold, skipping"
                    );
                    return None;
                }

                // Convert max trade size from ETH to wei (1 ETH = 1e18 wei)
                let amount_wei = (self.max_trade_size_eth * 1e18) as u128;
                // Estimate minimum output accounting for spread
                let min_out_wei =
                    (amount_wei as f64 * (1.0 - spread_bps / 10_000.0)) as u128;

                Some(TradeIntent {
                    strategy_name: self.name().to_string(),
                    token_in: Address::zero(),
                    token_out: Address::zero(),
                    amount_in_wei: amount_wei.to_string(),
                    min_amount_out_wei: min_out_wei.to_string(),
                    dex: *pair_a_dex,
                    side: TradeSide::Buy,
                    expected_profit_bps: *spread_bps,
                    deadline_secs: 120,
                    max_gas_price_gwei: None,
                })
            }
            _ => {
                debug!(signal_type = ?signal.signal_type, "ignoring non-divergence signal");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Dex;

    fn make_divergence_signal(spread_bps: f64) -> MarketSignal {
        MarketSignal {
            signal_type: SignalType::PriceDivergence {
                pair_a_dex: Dex::UniswapV2,
                pair_b_dex: Dex::SushiSwap,
                spread_bps,
            },
            quotes: vec![],
            source_tx: None,
        }
    }

    #[tokio::test]
    async fn test_returns_intent_when_spread_above_threshold() {
        let strategy = ArbitrageStrategy::new(50.0, 10.0);
        let signal = make_divergence_signal(100.0);

        let result = strategy.evaluate(&signal).await;
        assert!(result.is_some());

        let intent = result.unwrap();
        assert_eq!(intent.strategy_name, "arbitrage");
        assert_eq!(intent.expected_profit_bps, 100.0);
        assert_eq!(intent.dex, Dex::UniswapV2);
        assert_eq!(intent.side, TradeSide::Buy);
        assert_eq!(intent.deadline_secs, 120);
    }

    #[tokio::test]
    async fn test_returns_none_when_spread_below_threshold() {
        let strategy = ArbitrageStrategy::new(50.0, 10.0);
        let signal = make_divergence_signal(30.0);

        let result = strategy.evaluate(&signal).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_returns_none_for_non_divergence_signal() {
        let strategy = ArbitrageStrategy::new(50.0, 10.0);
        let signal = MarketSignal {
            signal_type: SignalType::NewBlock { block_number: 12345 },
            quotes: vec![],
            source_tx: None,
        };

        let result = strategy.evaluate(&signal).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_returns_intent_at_exact_threshold() {
        let strategy = ArbitrageStrategy::new(50.0, 10.0);
        let signal = make_divergence_signal(50.0);

        let result = strategy.evaluate(&signal).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_trade_size_in_wei() {
        let strategy = ArbitrageStrategy::new(50.0, 5.0);
        let signal = make_divergence_signal(100.0);

        let intent = strategy.evaluate(&signal).await.unwrap();
        // 5.0 ETH = 5_000_000_000_000_000_000 wei
        assert_eq!(intent.amount_in_wei, "5000000000000000000");
    }
}
