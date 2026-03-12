use std::sync::RwLock;

/// Tracks current portfolio exposure and daily realized losses.
///
/// Thread-safe via `RwLock` so multiple tasks can read exposure
/// while the agent loop updates it.
pub struct ExposureTracker {
    exposure_eth: RwLock<f64>,
    daily_loss_eth: RwLock<f64>,
}

impl ExposureTracker {
    pub fn new() -> Self {
        Self {
            exposure_eth: RwLock::new(0.0),
            daily_loss_eth: RwLock::new(0.0),
        }
    }

    /// Increase tracked exposure by `amount_eth`.
    pub fn add_exposure(&self, amount_eth: f64) {
        let mut exp = self.exposure_eth.write().unwrap();
        *exp += amount_eth;
    }

    /// Decrease tracked exposure by `amount_eth`.
    pub fn remove_exposure(&self, amount_eth: f64) {
        let mut exp = self.exposure_eth.write().unwrap();
        *exp = (*exp - amount_eth).max(0.0);
    }

    /// Current total exposure in ETH.
    pub fn current_exposure(&self) -> f64 {
        *self.exposure_eth.read().unwrap()
    }

    /// Record a realized loss.
    pub fn record_loss(&self, amount_eth: f64) {
        let mut loss = self.daily_loss_eth.write().unwrap();
        *loss += amount_eth;
    }

    /// Accumulated daily loss in ETH.
    pub fn daily_loss(&self) -> f64 {
        *self.daily_loss_eth.read().unwrap()
    }

    /// Reset daily loss counter (called at day boundary).
    pub fn reset_daily(&self) {
        let mut loss = self.daily_loss_eth.write().unwrap();
        *loss = 0.0;
    }
}

impl Default for ExposureTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_remove_exposure() {
        let tracker = ExposureTracker::new();
        assert_eq!(tracker.current_exposure(), 0.0);

        tracker.add_exposure(5.0);
        assert_eq!(tracker.current_exposure(), 5.0);

        tracker.add_exposure(3.0);
        assert_eq!(tracker.current_exposure(), 8.0);

        tracker.remove_exposure(2.0);
        assert_eq!(tracker.current_exposure(), 6.0);
    }

    #[test]
    fn test_remove_exposure_floors_at_zero() {
        let tracker = ExposureTracker::new();
        tracker.add_exposure(1.0);
        tracker.remove_exposure(5.0);
        assert_eq!(tracker.current_exposure(), 0.0);
    }

    #[test]
    fn test_daily_loss_tracking() {
        let tracker = ExposureTracker::new();
        assert_eq!(tracker.daily_loss(), 0.0);

        tracker.record_loss(0.5);
        tracker.record_loss(0.3);
        assert!((tracker.daily_loss() - 0.8).abs() < 1e-10);
    }

    #[test]
    fn test_reset_daily() {
        let tracker = ExposureTracker::new();
        tracker.record_loss(1.0);
        assert_eq!(tracker.daily_loss(), 1.0);

        tracker.reset_daily();
        assert_eq!(tracker.daily_loss(), 0.0);
    }
}
