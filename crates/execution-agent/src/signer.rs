use async_trait::async_trait;
use common::errors::ExecutionError;
use common::types::Address;

/// Trait for signing raw transaction bytes.
#[async_trait]
pub trait Signer: Send + Sync {
    async fn sign(&self, tx_bytes: &[u8]) -> Result<Vec<u8>, ExecutionError>;
    fn address(&self) -> Address;
}

/// Signer that reads a private key from the `PRIVATE_KEY` environment variable.
///
/// For this simplified implementation, the "private key" is treated as raw bytes
/// and signing appends a simple hash-like suffix. In production, this would use
/// proper ECDSA signing.
pub struct LocalSigner {
    key_bytes: Vec<u8>,
    addr: Address,
}

impl LocalSigner {
    /// Create a new `LocalSigner` by reading `PRIVATE_KEY` from the environment.
    pub fn from_env() -> Result<Self, ExecutionError> {
        let key_hex = std::env::var("PRIVATE_KEY").map_err(|_| {
            ExecutionError::SigningError("PRIVATE_KEY env var not set".to_string())
        })?;
        let key_hex = key_hex.strip_prefix("0x").unwrap_or(&key_hex);
        let key_bytes = hex_decode(key_hex).map_err(|e| {
            ExecutionError::SigningError(format!("invalid hex in PRIVATE_KEY: {e}"))
        })?;

        // Derive a deterministic address from the key (simplified: use first 20 bytes or hash).
        let mut addr_bytes = [0u8; 20];
        for (i, &b) in key_bytes.iter().enumerate() {
            addr_bytes[i % 20] ^= b;
        }
        let addr = Address(addr_bytes);

        Ok(Self { key_bytes, addr })
    }

    /// Create a `LocalSigner` directly from a hex key string (for testing).
    pub fn from_key_hex(key_hex: &str) -> Result<Self, ExecutionError> {
        let key_hex = key_hex.strip_prefix("0x").unwrap_or(key_hex);
        let key_bytes = hex_decode(key_hex).map_err(|e| {
            ExecutionError::SigningError(format!("invalid hex: {e}"))
        })?;

        let mut addr_bytes = [0u8; 20];
        for (i, &b) in key_bytes.iter().enumerate() {
            addr_bytes[i % 20] ^= b;
        }

        Ok(Self {
            key_bytes,
            addr: Address(addr_bytes),
        })
    }
}

#[async_trait]
impl Signer for LocalSigner {
    async fn sign(&self, tx_bytes: &[u8]) -> Result<Vec<u8>, ExecutionError> {
        // Simplified signing: XOR key bytes over the tx bytes to produce a "signature".
        // In production, this would be ECDSA over secp256k1.
        let mut signed = tx_bytes.to_vec();
        for (i, b) in signed.iter_mut().enumerate() {
            *b ^= self.key_bytes[i % self.key_bytes.len()];
        }
        // Append a fixed-length signature marker
        signed.extend_from_slice(b"SIG");
        Ok(signed)
    }

    fn address(&self) -> Address {
        self.addr
    }
}

/// Mock signer for testing — returns a deterministic dummy signature.
pub struct MockSigner {
    addr: Address,
}

impl MockSigner {
    pub fn new(addr: Address) -> Self {
        Self { addr }
    }
}

#[async_trait]
impl Signer for MockSigner {
    async fn sign(&self, tx_bytes: &[u8]) -> Result<Vec<u8>, ExecutionError> {
        let mut signed = Vec::with_capacity(tx_bytes.len() + 4);
        signed.extend_from_slice(tx_bytes);
        signed.extend_from_slice(b"MOCK");
        Ok(signed)
    }

    fn address(&self) -> Address {
        self.addr
    }
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex string".to_string());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| format!("invalid hex at position {i}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_signer_returns_dummy_signature() {
        let signer = MockSigner::new(Address::zero());
        let data = b"hello";
        let signed = signer.sign(data).await.unwrap();
        assert!(signed.ends_with(b"MOCK"));
        assert_eq!(&signed[..5], b"hello");
    }

    #[tokio::test]
    async fn test_mock_signer_address() {
        let addr = Address([1u8; 20]);
        let signer = MockSigner::new(addr);
        assert_eq!(signer.address(), addr);
    }

    #[tokio::test]
    async fn test_local_signer_from_key_hex() {
        let signer = LocalSigner::from_key_hex("deadbeef").unwrap();
        let data = b"test tx";
        let signed = signer.sign(data).await.unwrap();
        assert!(signed.ends_with(b"SIG"));
        assert_eq!(signed.len(), data.len() + 3);
    }

    #[tokio::test]
    async fn test_local_signer_deterministic() {
        let signer = LocalSigner::from_key_hex("abcd1234").unwrap();
        let data = b"some transaction";
        let sig1 = signer.sign(data).await.unwrap();
        let sig2 = signer.sign(data).await.unwrap();
        assert_eq!(sig1, sig2);
    }
}
