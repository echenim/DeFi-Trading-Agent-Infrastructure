use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommonError {
    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("invalid tx hash: {0}")]
    InvalidTxHash(String),

    #[error("hex decode error: {0}")]
    HexDecode(String),

    #[error("config error: {0}")]
    Config(String),
}

#[derive(Error, Debug)]
pub enum RpcError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("subscription error: {0}")]
    SubscriptionError(String),

    #[error("request timeout after {0}ms")]
    Timeout(u64),

    #[error("provider error: {0}")]
    ProviderError(String),
}

#[derive(Error, Debug)]
pub enum MessageBusError {
    #[error("send failed: {0}")]
    SendFailed(String),

    #[error("channel closed")]
    ChannelClosed,

    #[error("subscriber lagged, missed {0} messages")]
    SubscriberLagged(u64),
}

#[derive(Error, Debug)]
pub enum StrategyError {
    #[error("evaluation failed: {0}")]
    EvaluationFailed(String),

    #[error("insufficient data for strategy: {0}")]
    InsufficientData(String),
}

#[derive(Error, Debug)]
pub enum RiskError {
    #[error("exposure limit exceeded: current={current_eth:.4} max={max_eth:.4}")]
    ExposureLimitExceeded { current_eth: f64, max_eth: f64 },

    #[error("slippage too high: {actual_bps}bps > {max_bps}bps")]
    SlippageTooHigh { actual_bps: u64, max_bps: u64 },

    #[error("gas price too high: {price_gwei:.2} gwei > {max_gwei:.2} gwei")]
    GasPriceTooHigh { price_gwei: f64, max_gwei: f64 },

    #[error("daily loss limit reached: {loss_eth:.4} ETH")]
    DailyLossLimitReached { loss_eth: f64 },

    #[error("sanity check failed: {0}")]
    SanityCheckFailed(String),
}

#[derive(Error, Debug)]
pub enum ExecutionError {
    #[error("nonce error: {0}")]
    NonceError(String),

    #[error("gas estimation failed: {0}")]
    GasEstimationFailed(String),

    #[error("signing error: {0}")]
    SigningError(String),

    #[error("broadcast failed: {0}")]
    BroadcastFailed(String),

    #[error("tx not confirmed within {timeout_secs}s")]
    ConfirmationTimeout { timeout_secs: u64 },

    #[error("tx reverted: {tx_hash}")]
    TxReverted { tx_hash: String },
}
