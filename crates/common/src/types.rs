use serde::{Deserialize, Serialize};
use std::fmt;

/// Ethereum address as a 20-byte array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(pub [u8; 20]);

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x")?;
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Address {
    pub fn zero() -> Self {
        Self([0u8; 20])
    }

    pub fn from_hex(s: &str) -> Result<Self, crate::errors::CommonError> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        if s.len() != 40 {
            return Err(crate::errors::CommonError::InvalidAddress(s.to_string()));
        }
        let bytes = hex_decode(s)?;
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&bytes);
        Ok(Self(addr))
    }
}

/// Transaction hash as a 32-byte array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxHash(pub [u8; 32]);

impl fmt::Display for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x")?;
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl TxHash {
    pub fn from_hex(s: &str) -> Result<Self, crate::errors::CommonError> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        if s.len() != 64 {
            return Err(crate::errors::CommonError::InvalidTxHash(s.to_string()));
        }
        let bytes = hex_decode(s)?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes);
        Ok(Self(hash))
    }
}

/// DEX identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dex {
    UniswapV2,
    UniswapV3,
    SushiSwap,
}

impl fmt::Display for Dex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dex::UniswapV2 => write!(f, "UniswapV2"),
            Dex::UniswapV3 => write!(f, "UniswapV3"),
            Dex::SushiSwap => write!(f, "SushiSwap"),
        }
    }
}

/// A token pair on a DEX.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenPair {
    pub token_a: Address,
    pub token_b: Address,
    pub dex: Dex,
}

/// Price quote for a token pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceQuote {
    pub pair: TokenPair,
    pub price: f64,
    pub liquidity: f64,
    pub timestamp_ms: u64,
}

/// Side of a trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

fn hex_decode(s: &str) -> Result<Vec<u8>, crate::errors::CommonError> {
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| crate::errors::CommonError::HexDecode(s.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_roundtrip() {
        let hex = "0xdead00000000000000000000000000000000beef";
        let addr = Address::from_hex(hex).unwrap();
        assert_eq!(addr.to_string(), hex);
    }

    #[test]
    fn test_tx_hash_roundtrip() {
        let hex = "0x0000000000000000000000000000000000000000000000000000000000000001";
        let hash = TxHash::from_hex(hex).unwrap();
        assert_eq!(hash.to_string(), hex);
    }

    #[test]
    fn test_invalid_address() {
        assert!(Address::from_hex("0xshort").is_err());
    }
}
