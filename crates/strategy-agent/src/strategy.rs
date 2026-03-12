use async_trait::async_trait;
use common::messages::{MarketSignal, TradeIntent};

/// Trait that all trading strategies must implement.
///
/// Each strategy evaluates a market signal and optionally returns a trade intent.
#[async_trait]
pub trait Strategy: Send + Sync {
    /// Human-readable name for this strategy.
    fn name(&self) -> &str;

    /// Evaluate a market signal and return a trade intent if an opportunity is detected.
    async fn evaluate(&self, signal: &MarketSignal) -> Option<TradeIntent>;
}
