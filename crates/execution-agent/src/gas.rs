/// Simple deterministic gas estimation utilities.
pub struct GasEstimator;

impl GasEstimator {
    /// Apply a multiplier to a base gas estimate.
    ///
    /// This adds a safety margin to prevent out-of-gas failures.
    pub fn estimate_gas(base_gas: u64, multiplier: f64) -> u64 {
        (base_gas as f64 * multiplier) as u64
    }

    /// Calculate the total gas price from base fee and priority fee.
    ///
    /// Returns `base_fee_gwei + priority_fee_gwei` (EIP-1559 style).
    pub fn apply_priority_fee(base_fee_gwei: f64, priority_fee_gwei: f64) -> f64 {
        base_fee_gwei + priority_fee_gwei
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_gas_with_multiplier() {
        assert_eq!(GasEstimator::estimate_gas(21000, 1.0), 21000);
        assert_eq!(GasEstimator::estimate_gas(21000, 1.2), 25200);
        assert_eq!(GasEstimator::estimate_gas(100_000, 1.5), 150_000);
    }

    #[test]
    fn test_estimate_gas_multiplier_less_than_one() {
        // Edge case: multiplier < 1 (shouldn't happen in practice but should work)
        assert_eq!(GasEstimator::estimate_gas(100_000, 0.5), 50_000);
    }

    #[test]
    fn test_apply_priority_fee() {
        let total = GasEstimator::apply_priority_fee(30.0, 2.0);
        assert!((total - 32.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_apply_priority_fee_zero() {
        let total = GasEstimator::apply_priority_fee(30.0, 0.0);
        assert!((total - 30.0).abs() < f64::EPSILON);
    }
}
