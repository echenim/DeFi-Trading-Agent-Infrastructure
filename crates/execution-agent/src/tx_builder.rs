use common::messages::ValidatedTrade;
use common::types::Address;
use serde::{Deserialize, Serialize};

/// A raw transaction ready to be signed and broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTransaction {
    pub to: Address,
    pub value: String,
    pub data: Vec<u8>,
    pub nonce: u64,
    pub gas_limit: u64,
    pub gas_price_gwei: f64,
    pub chain_id: u64,
}

impl RawTransaction {
    /// Encode the transaction into bytes (simplified encoding).
    ///
    /// In production this would be RLP-encoded. For now we use a deterministic
    /// JSON serialization so the output is predictable and testable.
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("RawTransaction serialization should not fail")
    }
}

/// Builds `RawTransaction` values from validated trades.
pub struct TransactionBuilder {
    chain_id: u64,
}

impl TransactionBuilder {
    pub fn new(chain_id: u64) -> Self {
        Self { chain_id }
    }

    /// Build a `RawTransaction` from a validated trade.
    ///
    /// The `to` address is derived from the DEX router (simplified: uses `token_out`
    /// as the target). Calldata is constructed from the trade intent fields.
    pub fn build(
        &self,
        trade: &ValidatedTrade,
        nonce: u64,
        gas_limit: u64,
        gas_price_gwei: f64,
    ) -> RawTransaction {
        let intent = &trade.intent;

        // Simplified calldata: encode swap parameters as JSON bytes.
        // In production, this would be ABI-encoded function call data.
        let calldata = serde_json::json!({
            "function": "swap",
            "token_in": format!("{}", intent.token_in),
            "token_out": format!("{}", intent.token_out),
            "amount_in": &intent.amount_in_wei,
            "min_amount_out": &intent.min_amount_out_wei,
            "dex": format!("{}", intent.dex),
        });

        RawTransaction {
            to: intent.token_out,
            value: intent.amount_in_wei.clone(),
            data: serde_json::to_vec(&calldata).unwrap_or_default(),
            nonce,
            gas_limit,
            gas_price_gwei,
            chain_id: self.chain_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::messages::{TradeIntent, ValidatedTrade};
    use common::types::{Address, Dex, TradeSide};

    fn sample_validated_trade() -> ValidatedTrade {
        ValidatedTrade {
            intent: TradeIntent {
                strategy_name: "arb_v1".to_string(),
                token_in: Address::zero(),
                token_out: Address([0xAA; 20]),
                amount_in_wei: "1000000000000000000".to_string(),
                min_amount_out_wei: "990000000000000000".to_string(),
                dex: Dex::UniswapV2,
                side: TradeSide::Buy,
                expected_profit_bps: 75.0,
                deadline_secs: 120,
                max_gas_price_gwei: Some(50.0),
            },
            risk_score: 0.3,
            approved_at_ms: 1700000000000,
        }
    }

    #[test]
    fn test_build_transaction() {
        let builder = TransactionBuilder::new(1);
        let trade = sample_validated_trade();
        let tx = builder.build(&trade, 5, 200_000, 30.0);

        assert_eq!(tx.nonce, 5);
        assert_eq!(tx.gas_limit, 200_000);
        assert!((tx.gas_price_gwei - 30.0).abs() < f64::EPSILON);
        assert_eq!(tx.chain_id, 1);
        assert_eq!(tx.to, Address([0xAA; 20]));
        assert_eq!(tx.value, "1000000000000000000");
        assert!(!tx.data.is_empty());
    }

    #[test]
    fn test_encode_deterministic() {
        let builder = TransactionBuilder::new(1);
        let trade = sample_validated_trade();
        let tx = builder.build(&trade, 0, 21000, 20.0);

        let enc1 = tx.encode();
        let enc2 = tx.encode();
        assert_eq!(enc1, enc2);
        assert!(!enc1.is_empty());
    }

    #[test]
    fn test_encode_deserializes_back() {
        let builder = TransactionBuilder::new(5);
        let trade = sample_validated_trade();
        let tx = builder.build(&trade, 10, 100_000, 45.0);
        let bytes = tx.encode();

        let decoded: RawTransaction = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.nonce, 10);
        assert_eq!(decoded.chain_id, 5);
    }
}
