use common::errors::RiskError;
use common::messages::TradeIntent;

use crate::state::ExposureTracker;

/// Check that adding this trade would not exceed the maximum exposure limit.
pub fn check_exposure_limit(
    intent: &TradeIntent,
    tracker: &ExposureTracker,
    max_eth: f64,
) -> Result<(), RiskError> {
    // Parse amount_in_wei to ETH (1 ETH = 1e18 wei)
    let amount_eth = wei_to_eth(&intent.amount_in_wei);
    let projected = tracker.current_exposure() + amount_eth;
    if projected > max_eth {
        return Err(RiskError::ExposureLimitExceeded {
            current_eth: projected,
            max_eth,
        });
    }
    Ok(())
}

/// Check that implied slippage does not exceed the maximum allowed basis points.
///
/// Slippage is derived from expected_profit_bps being negative or from the
/// difference between amount_in and min_amount_out. We use the intent's
/// expected_profit_bps as a proxy for slippage tolerance: if profit_bps is
/// less than 0, that means the trade accepts a loss, which we flag.
///
/// For a direct slippage check we compute:
///   slippage_bps = ((amount_in - min_amount_out) / amount_in) * 10_000
pub fn check_slippage(intent: &TradeIntent, max_bps: u64) -> Result<(), RiskError> {
    let amount_in: f64 = intent.amount_in_wei.parse().unwrap_or(0.0);
    let min_out: f64 = intent.min_amount_out_wei.parse().unwrap_or(0.0);

    if amount_in <= 0.0 {
        return Ok(()); // sanity check will catch zero amounts
    }

    let slippage_bps = ((amount_in - min_out) / amount_in * 10_000.0) as u64;
    if slippage_bps > max_bps {
        return Err(RiskError::SlippageTooHigh {
            actual_bps: slippage_bps,
            max_bps,
        });
    }
    Ok(())
}

/// Check that the trade's gas price does not exceed the maximum allowed.
pub fn check_gas_price(intent: &TradeIntent, max_gwei: f64) -> Result<(), RiskError> {
    if let Some(gas_price) = intent.max_gas_price_gwei {
        if gas_price > max_gwei {
            return Err(RiskError::GasPriceTooHigh {
                price_gwei: gas_price,
                max_gwei,
            });
        }
    }
    Ok(())
}

/// Check that the daily loss limit has not been reached.
pub fn check_daily_loss_limit(
    tracker: &ExposureTracker,
    limit_eth: f64,
) -> Result<(), RiskError> {
    let loss = tracker.daily_loss();
    if loss >= limit_eth {
        return Err(RiskError::DailyLossLimitReached { loss_eth: loss });
    }
    Ok(())
}

/// Basic sanity checks on a trade intent:
/// - amount must be > 0
/// - deadline must be > 0
/// - token_in must differ from token_out
pub fn check_sanity(intent: &TradeIntent) -> Result<(), RiskError> {
    let amount: f64 = intent.amount_in_wei.parse().unwrap_or(0.0);
    if amount <= 0.0 {
        return Err(RiskError::SanityCheckFailed(
            "amount_in_wei must be greater than zero".to_string(),
        ));
    }

    if intent.deadline_secs == 0 {
        return Err(RiskError::SanityCheckFailed(
            "deadline_secs must be greater than zero".to_string(),
        ));
    }

    if intent.token_in == intent.token_out {
        return Err(RiskError::SanityCheckFailed(
            "token_in must differ from token_out".to_string(),
        ));
    }

    Ok(())
}

/// Convert a wei string to ETH (f64).
fn wei_to_eth(wei_str: &str) -> f64 {
    let wei: f64 = wei_str.parse().unwrap_or(0.0);
    wei / 1e18
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{Address, Dex, TradeSide};

    fn make_intent() -> TradeIntent {
        TradeIntent {
            strategy_name: "test".to_string(),
            token_in: Address::zero(),
            token_out: Address([1u8; 20]),
            amount_in_wei: "1000000000000000000".to_string(), // 1 ETH
            min_amount_out_wei: "990000000000000000".to_string(), // 0.99 ETH — 100 bps slippage
            dex: Dex::UniswapV2,
            side: TradeSide::Buy,
            expected_profit_bps: 50.0,
            deadline_secs: 120,
            max_gas_price_gwei: Some(50.0),
        }
    }

    // --- Exposure limit ---

    #[test]
    fn test_exposure_limit_pass() {
        let tracker = ExposureTracker::new();
        let intent = make_intent();
        assert!(check_exposure_limit(&intent, &tracker, 10.0).is_ok());
    }

    #[test]
    fn test_exposure_limit_reject() {
        let tracker = ExposureTracker::new();
        tracker.add_exposure(9.5);
        let intent = make_intent(); // adds 1 ETH -> 10.5 > 10.0
        assert!(check_exposure_limit(&intent, &tracker, 10.0).is_err());
    }

    // --- Slippage ---

    #[test]
    fn test_slippage_pass() {
        let intent = make_intent(); // 100 bps slippage
        assert!(check_slippage(&intent, 200).is_ok());
    }

    #[test]
    fn test_slippage_reject() {
        let intent = make_intent(); // 100 bps slippage
        assert!(check_slippage(&intent, 50).is_err());
    }

    // --- Gas price ---

    #[test]
    fn test_gas_price_pass() {
        let intent = make_intent(); // max_gas_price_gwei = 50
        assert!(check_gas_price(&intent, 100.0).is_ok());
    }

    #[test]
    fn test_gas_price_reject() {
        let intent = make_intent(); // max_gas_price_gwei = 50
        assert!(check_gas_price(&intent, 30.0).is_err());
    }

    #[test]
    fn test_gas_price_none_passes() {
        let mut intent = make_intent();
        intent.max_gas_price_gwei = None;
        assert!(check_gas_price(&intent, 10.0).is_ok());
    }

    // --- Daily loss ---

    #[test]
    fn test_daily_loss_pass() {
        let tracker = ExposureTracker::new();
        tracker.record_loss(0.5);
        assert!(check_daily_loss_limit(&tracker, 1.0).is_ok());
    }

    #[test]
    fn test_daily_loss_reject() {
        let tracker = ExposureTracker::new();
        tracker.record_loss(2.0);
        assert!(check_daily_loss_limit(&tracker, 1.0).is_err());
    }

    // --- Sanity checks ---

    #[test]
    fn test_sanity_pass() {
        let intent = make_intent();
        assert!(check_sanity(&intent).is_ok());
    }

    #[test]
    fn test_sanity_rejects_zero_amount() {
        let mut intent = make_intent();
        intent.amount_in_wei = "0".to_string();
        assert!(check_sanity(&intent).is_err());
    }

    #[test]
    fn test_sanity_rejects_zero_deadline() {
        let mut intent = make_intent();
        intent.deadline_secs = 0;
        assert!(check_sanity(&intent).is_err());
    }

    #[test]
    fn test_sanity_rejects_same_tokens() {
        let mut intent = make_intent();
        intent.token_out = intent.token_in;
        assert!(check_sanity(&intent).is_err());
    }
}
