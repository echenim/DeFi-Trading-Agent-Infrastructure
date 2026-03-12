use serde::Deserialize;
use std::path::Path;

use crate::errors::CommonError;

#[derive(Debug, Deserialize, Clone)]
pub struct RpcConfig {
    pub ethereum: EthereumConfig,
    pub retry: RetryConfig,
    pub circuit_breaker: CircuitBreakerConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EthereumConfig {
    pub ws_url: String,
    pub http_url: String,
    pub chain_id: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub reset_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TradingConfig {
    pub market_data: MarketDataConfig,
    pub strategy: StrategyConfig,
    pub risk: RiskConfig,
    pub execution: ExecutionConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MarketDataConfig {
    pub mempool_buffer_size: usize,
    pub price_staleness_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StrategyConfig {
    pub min_profit_bps: f64,
    pub max_trade_size_eth: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RiskConfig {
    pub max_position_size_eth: f64,
    pub max_slippage_bps: u64,
    pub max_gas_price_gwei: f64,
    pub daily_loss_limit_eth: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExecutionConfig {
    pub gas_price_multiplier: f64,
    pub dry_run: bool,
    pub confirmation_blocks: u64,
    pub tx_timeout_secs: u64,
}

pub fn load_rpc_config(config_dir: &Path) -> Result<RpcConfig, CommonError> {
    let path = config_dir.join("rpc.toml");
    let content =
        std::fs::read_to_string(&path).map_err(|e| CommonError::Config(format!("{path:?}: {e}")))?;
    toml::from_str(&content).map_err(|e| CommonError::Config(format!("{path:?}: {e}")))
}

pub fn load_trading_config(config_dir: &Path) -> Result<TradingConfig, CommonError> {
    let path = config_dir.join("trading.toml");
    let content =
        std::fs::read_to_string(&path).map_err(|e| CommonError::Config(format!("{path:?}: {e}")))?;
    toml::from_str(&content).map_err(|e| CommonError::Config(format!("{path:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn configs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("configs")
    }

    #[test]
    fn test_load_rpc_config() {
        let config = load_rpc_config(&configs_dir()).unwrap();
        assert_eq!(config.ethereum.chain_id, 1);
        assert_eq!(config.retry.max_retries, 3);
        assert_eq!(config.circuit_breaker.failure_threshold, 5);
    }

    #[test]
    fn test_load_trading_config() {
        let config = load_trading_config(&configs_dir()).unwrap();
        assert_eq!(config.strategy.min_profit_bps, 50.0);
        assert!(config.execution.dry_run);
        assert_eq!(config.risk.max_slippage_bps, 100);
    }

    #[test]
    fn test_rpc_config_deserialize() {
        let toml_str = r#"
[ethereum]
ws_url = "ws://localhost:8545"
http_url = "http://localhost:8545"
chain_id = 5

[retry]
max_retries = 5
initial_backoff_ms = 200
max_backoff_ms = 10000

[circuit_breaker]
failure_threshold = 10
reset_timeout_secs = 60
"#;
        let config: RpcConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.ethereum.chain_id, 5);
        assert_eq!(config.retry.max_retries, 5);
    }
}
