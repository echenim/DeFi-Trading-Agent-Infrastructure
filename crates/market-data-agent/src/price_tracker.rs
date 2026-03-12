use common::types::{Dex, PriceQuote, TokenPair};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Thread-safe price tracker that stores latest quotes per token pair
/// and detects price divergences across DEXes.
#[derive(Clone)]
pub struct PriceTracker {
    prices: Arc<RwLock<HashMap<TokenPair, PriceQuote>>>,
}

impl PriceTracker {
    pub fn new() -> Self {
        Self {
            prices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Update (or insert) the price for a token pair.
    pub async fn update_price(&self, quote: PriceQuote) {
        let mut prices = self.prices.write().await;
        prices.insert(quote.pair.clone(), quote);
    }

    /// Get the current price for a token pair.
    pub async fn get_price(&self, pair: &TokenPair) -> Option<PriceQuote> {
        let prices = self.prices.read().await;
        prices.get(pair).cloned()
    }

    /// Detect price divergence between two DEXes for the same token addresses.
    ///
    /// Looks up the price for the pair on `dex_a` and `dex_b`.
    /// If both exist and the spread in basis points exceeds `threshold_bps`,
    /// returns the spread. Otherwise returns `None`.
    ///
    /// Spread is calculated as: `|price_a - price_b| / min(price_a, price_b) * 10_000`
    pub async fn detect_divergence(
        &self,
        pair: &TokenPair,
        dex_a: Dex,
        dex_b: Dex,
        threshold_bps: f64,
    ) -> Option<f64> {
        let prices = self.prices.read().await;

        let pair_a = TokenPair {
            token_a: pair.token_a,
            token_b: pair.token_b,
            dex: dex_a,
        };
        let pair_b = TokenPair {
            token_a: pair.token_a,
            token_b: pair.token_b,
            dex: dex_b,
        };

        let quote_a = prices.get(&pair_a)?;
        let quote_b = prices.get(&pair_b)?;

        let price_a = quote_a.price;
        let price_b = quote_b.price;

        if price_a <= 0.0 || price_b <= 0.0 {
            return None;
        }

        let min_price = price_a.min(price_b);
        let spread_bps = (price_a - price_b).abs() / min_price * 10_000.0;

        if spread_bps > threshold_bps {
            Some(spread_bps)
        } else {
            None
        }
    }
}

impl Default for PriceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Address;

    fn weth() -> Address {
        Address([0x11; 20])
    }

    fn usdc() -> Address {
        Address([0x22; 20])
    }

    fn make_pair(dex: Dex) -> TokenPair {
        TokenPair {
            token_a: weth(),
            token_b: usdc(),
            dex,
        }
    }

    fn make_quote(dex: Dex, price: f64) -> PriceQuote {
        PriceQuote {
            pair: make_pair(dex),
            price,
            liquidity: 1_000_000.0,
            timestamp_ms: 1700000000000,
        }
    }

    #[tokio::test]
    async fn test_update_and_get_price() {
        let tracker = PriceTracker::new();
        let quote = make_quote(Dex::UniswapV2, 1800.0);

        tracker.update_price(quote.clone()).await;
        let retrieved = tracker.get_price(&make_pair(Dex::UniswapV2)).await;

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().price, 1800.0);
    }

    #[tokio::test]
    async fn test_get_nonexistent_price() {
        let tracker = PriceTracker::new();
        let result = tracker.get_price(&make_pair(Dex::UniswapV3)).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_overwrites_price() {
        let tracker = PriceTracker::new();

        tracker.update_price(make_quote(Dex::UniswapV2, 1800.0)).await;
        tracker.update_price(make_quote(Dex::UniswapV2, 1850.0)).await;

        let retrieved = tracker.get_price(&make_pair(Dex::UniswapV2)).await;
        assert_eq!(retrieved.unwrap().price, 1850.0);
    }

    #[tokio::test]
    async fn test_detect_divergence_above_threshold() {
        let tracker = PriceTracker::new();

        // UniswapV2: 1800.0, SushiSwap: 1820.0
        // Spread = |1800 - 1820| / 1800 * 10000 = 111.11 bps
        tracker.update_price(make_quote(Dex::UniswapV2, 1800.0)).await;
        tracker.update_price(make_quote(Dex::SushiSwap, 1820.0)).await;

        // Use a "template" pair (dex field doesn't matter for the lookup key,
        // since detect_divergence builds its own pairs).
        let pair = make_pair(Dex::UniswapV2);

        let result = tracker
            .detect_divergence(&pair, Dex::UniswapV2, Dex::SushiSwap, 50.0)
            .await;

        assert!(result.is_some());
        let spread = result.unwrap();
        assert!(spread > 100.0, "expected ~111 bps, got {spread}");
        assert!(spread < 120.0, "expected ~111 bps, got {spread}");
    }

    #[tokio::test]
    async fn test_detect_divergence_below_threshold() {
        let tracker = PriceTracker::new();

        // Very small spread: 1800 vs 1801 => ~5.5 bps
        tracker.update_price(make_quote(Dex::UniswapV2, 1800.0)).await;
        tracker.update_price(make_quote(Dex::SushiSwap, 1801.0)).await;

        let pair = make_pair(Dex::UniswapV2);
        let result = tracker
            .detect_divergence(&pair, Dex::UniswapV2, Dex::SushiSwap, 50.0)
            .await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_detect_divergence_missing_dex() {
        let tracker = PriceTracker::new();

        tracker.update_price(make_quote(Dex::UniswapV2, 1800.0)).await;
        // No SushiSwap price

        let pair = make_pair(Dex::UniswapV2);
        let result = tracker
            .detect_divergence(&pair, Dex::UniswapV2, Dex::SushiSwap, 50.0)
            .await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_detect_divergence_exact_threshold() {
        let tracker = PriceTracker::new();

        // Spread of exactly 50 bps: 1800 vs 1809 => 50 bps
        // 1800 * 50/10000 = 9.0 => price_b = 1809
        tracker.update_price(make_quote(Dex::UniswapV2, 1800.0)).await;
        tracker.update_price(make_quote(Dex::SushiSwap, 1809.0)).await;

        let pair = make_pair(Dex::UniswapV2);

        // At exactly 50 bps, detect_divergence uses > (strict), so exactly 50 should return None
        let result = tracker
            .detect_divergence(&pair, Dex::UniswapV2, Dex::SushiSwap, 50.0)
            .await;

        assert!(result.is_none());
    }
}
