use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{Address, Dex, PriceQuote, TradeSide, TxHash};

/// Unique message identifier for idempotency.
pub type MessageId = Uuid;

/// Envelope wrapping every message on the bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub id: MessageId,
    pub timestamp_ms: u64,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn new(payload: T) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            payload,
        }
    }
}

/// Signal emitted by the Market Data Agent when it detects a potential opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSignal {
    pub signal_type: SignalType,
    pub quotes: Vec<PriceQuote>,
    pub source_tx: Option<TxHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalType {
    PriceDivergence {
        pair_a_dex: Dex,
        pair_b_dex: Dex,
        spread_bps: f64,
    },
    LargeSwap {
        dex: Dex,
        value_eth: f64,
    },
    NewBlock {
        block_number: u64,
    },
}

/// Intent to trade, emitted by a Strategy Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeIntent {
    pub strategy_name: String,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in_wei: String,
    pub min_amount_out_wei: String,
    pub dex: Dex,
    pub side: TradeSide,
    pub expected_profit_bps: f64,
    pub deadline_secs: u64,
    pub max_gas_price_gwei: Option<f64>,
}

/// Trade that passed risk validation, ready for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedTrade {
    pub intent: TradeIntent,
    pub risk_score: f64,
    pub approved_at_ms: u64,
}

/// Result of trade execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub trade: ValidatedTrade,
    pub outcome: ExecutionOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionOutcome {
    Success {
        tx_hash: TxHash,
        gas_used: u64,
        effective_gas_price_gwei: f64,
    },
    Failed {
        reason: String,
    },
    DryRun {
        would_send: String,
    },
}

/// Top-level message enum for the bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Signal(Envelope<MarketSignal>),
    Intent(Envelope<TradeIntent>),
    Validated(Envelope<ValidatedTrade>),
    Executed(Envelope<ExecutionResult>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Address, Dex, PriceQuote, TokenPair};

    #[test]
    fn test_message_serde_roundtrip() {
        let signal = MarketSignal {
            signal_type: SignalType::PriceDivergence {
                pair_a_dex: Dex::UniswapV2,
                pair_b_dex: Dex::SushiSwap,
                spread_bps: 75.0,
            },
            quotes: vec![PriceQuote {
                pair: TokenPair {
                    token_a: Address::zero(),
                    token_b: Address::zero(),
                    dex: Dex::UniswapV2,
                },
                price: 1800.50,
                liquidity: 1000.0,
                timestamp_ms: 1700000000000,
            }],
            source_tx: None,
        };

        let msg = Message::Signal(Envelope::new(signal));
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        match deserialized {
            Message::Signal(env) => {
                assert!(matches!(
                    env.payload.signal_type,
                    SignalType::PriceDivergence { .. }
                ));
            }
            _ => panic!("expected Signal"),
        }
    }

    #[test]
    fn test_trade_intent_serde() {
        let intent = TradeIntent {
            strategy_name: "arb_v1".to_string(),
            token_in: Address::zero(),
            token_out: Address::zero(),
            amount_in_wei: "1000000000000000000".to_string(),
            min_amount_out_wei: "990000000000000000".to_string(),
            dex: Dex::UniswapV3,
            side: TradeSide::Buy,
            expected_profit_bps: 50.0,
            deadline_secs: 120,
            max_gas_price_gwei: Some(100.0),
        };

        let json = serde_json::to_string(&intent).unwrap();
        let back: TradeIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.strategy_name, "arb_v1");
        assert_eq!(back.expected_profit_bps, 50.0);
    }

    #[test]
    fn test_envelope_has_unique_ids() {
        let e1 = Envelope::new(42u32);
        let e2 = Envelope::new(42u32);
        assert_ne!(e1.id, e2.id);
    }
}
