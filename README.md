# Agent-Based DeFi Trading Infrastructure

A modular Rust system for automated trading on decentralized finance networks. Four specialized async agents communicate via a broadcast message bus to process Ethereum mempool data, detect arbitrage opportunities, enforce risk constraints, and execute transactions deterministically.

```
Blockchain Mempool
        │
        ▼
Market Data Agent ── parse pending txs, detect DEX swaps, track prices
        │
        ▼
Strategy Agent ───── evaluate signals, generate trade intents (plugin-based)
        │
        ▼
Risk Agent ────────── validate exposure, slippage, gas, daily loss limits
        │
        ▼
Execution Agent ──── build tx, manage nonce, sign, broadcast (or dry-run)
        │
        ▼
Ethereum Network
```

---

## Quick Start

```bash
# Build
cargo build --workspace

# Run all tests (95 tests)
cargo test --workspace

# Start in dry-run mode (no real transactions)
RUST_LOG=info cargo run -p orchestrator -- --config-dir ./configs --dry-run

# Run benchmarks
cargo bench -p messaging -p market-data-agent
```

---

## Workspace Structure

```
crates/
  common/             Shared types, messages, config, errors
  messaging/          Async message bus, circuit breaker, retry with backoff
  market-data-agent/  Mempool RPC, Uniswap V2/V3 tx parser, price tracker
  strategy-agent/     Strategy trait + arbitrage strategy plugin
  risk-agent/         Trade validation rules, exposure tracking
  execution-agent/    Signing, nonce management, tx building, broadcast
  orchestrator/       Main binary — spawns all agents as Tokio tasks

configs/
  rpc.toml            RPC connection, retry, circuit breaker settings
  trading.toml        Strategy thresholds, risk limits, execution params
```

All agents are library crates. The `orchestrator` is the only binary — it wires agents together in a single Tokio runtime for local development. Agents communicate exclusively through the message bus; no shared state.

### Crate Dependency Graph

```
orchestrator → all agent crates → messaging → common
```

No agent crate depends on another agent crate.

---

## Architecture

### Message Pipeline

Every message flows through a typed enum on a broadcast channel:

```rust
enum Message {
    Signal(Envelope<MarketSignal>),      // Market Data → Strategy
    Intent(Envelope<TradeIntent>),       // Strategy → Risk
    Validated(Envelope<ValidatedTrade>), // Risk → Execution
    Executed(Envelope<ExecutionResult>), // Execution → (logging/monitoring)
}
```

Each `Envelope<T>` carries a UUID for idempotency and a millisecond timestamp. The bus uses `tokio::sync::broadcast` with configurable capacity (default 4096). Lagging subscribers skip missed messages rather than erroring.

### Market Data Agent

- Subscribes to Ethereum mempool via `RpcClientTrait` (WebSocket)
- Parses pending transactions for DEX swap calldata:
  - Uniswap V2: `swapExactTokensForTokens` (`0x38ed1738`), `swapTokensForExactTokens` (`0x8803dbee`)
  - Uniswap V3: `exactInputSingle` (`0x414bf389`)
- Tracks prices per `(token_pair, dex)` and detects cross-DEX divergence in basis points
- Publishes `MarketSignal` (LargeSwap or PriceDivergence) to the bus

### Strategy Agent

- Routes incoming `MarketSignal` messages to all registered strategies
- **Strategy trait**: `fn name()` + `async fn evaluate(&self, signal) -> Option<TradeIntent>`
- Built-in: `ArbitrageStrategy` — triggers on price divergence above a configurable BPS threshold
- Extensible: implement `Strategy` to add liquidation, yield, or custom logic

### Risk Agent

Validates every `TradeIntent` through a chain of checks before forwarding:

| Check | Rejects When |
|-------|-------------|
| Sanity | Zero amount, zero deadline, same token in/out |
| Exposure limit | Projected position exceeds `max_position_size_eth` |
| Slippage | Implied slippage exceeds `max_slippage_bps` |
| Gas price | `max_gas_price_gwei` on intent exceeds configured limit |
| Daily loss | Accumulated daily loss exceeds `daily_loss_limit_eth` |

Passing intents become `ValidatedTrade` with a risk score (weighted: 40% exposure, 30% gas, 30% loss ratio).

### Execution Agent

- **NonceManager**: atomic `u64` counter, `sync()` from chain on startup
- **GasEstimator**: base estimate with configurable multiplier
- **TransactionBuilder**: deterministic tx construction from `ValidatedTrade`
- **Signer trait**: `LocalSigner` (key from env) or `MockSigner` for testing
- **Broadcaster trait**: `DryRunBroadcaster` (logs only) or real broadcast
- Publishes `ExecutionResult` (Success, Failed, or DryRun) to the bus

### Resilience

- **Circuit breaker** (Closed → Open → HalfOpen) with configurable failure threshold and reset timeout
- **Exponential backoff retry** with configurable max retries and backoff bounds
- **CancellationToken** for graceful shutdown — agents drain their inbox before exiting
- **Idempotency** via UUID message IDs

---

## Configuration

### `configs/rpc.toml`

```toml
[ethereum]
ws_url = "ws://localhost:8545"
http_url = "http://localhost:8545"
chain_id = 1

[retry]
max_retries = 3
initial_backoff_ms = 100
max_backoff_ms = 5000

[circuit_breaker]
failure_threshold = 5
reset_timeout_secs = 30
```

### `configs/trading.toml`

```toml
[market_data]
mempool_buffer_size = 4096
price_staleness_secs = 30

[strategy]
min_profit_bps = 50           # 0.5% minimum profit
max_trade_size_eth = 10.0

[risk]
max_position_size_eth = 50.0
max_slippage_bps = 100        # 1%
max_gas_price_gwei = 200.0
daily_loss_limit_eth = 5.0

[execution]
gas_price_multiplier = 1.1
dry_run = true
confirmation_blocks = 1
tx_timeout_secs = 120
```

### Environment Variables

```bash
PRIVATE_KEY=<hex-encoded-private-key>   # for LocalSigner
RUST_LOG=info                            # tracing log level
```

See `.env.example` for a template. **Never commit real private keys.**

---

## Performance

Benchmarked with Criterion on local development hardware:

| Metric | Result |
|--------|--------|
| Message bus publish (1 subscriber) | **~40M ops/sec** (~24ns/op) |
| Message bus publish (4 subscribers) | **~38M ops/sec** (~25ns/op) |
| Publish → receive roundtrip | **~9.4M ops/sec** (~106ns/op) |
| Uniswap V2 swap parsing | **~83M ops/sec** (~12ns/op) |
| Swap selector detection | **~795M ops/sec** (~1.3ns/op) |

Run benchmarks: `cargo bench -p messaging -p market-data-agent`

---

## Testing

**95 tests** across 7 crates covering:

- Message serialization roundtrips
- Config deserialization from TOML files
- Circuit breaker state machine transitions
- Retry backoff behavior
- Transaction calldata parsing (Uniswap V2/V3)
- Price divergence detection
- All 5 risk validation rules (pass + reject cases)
- Nonce manager concurrency
- Execution agent dry-run and live modes
- End-to-end pipeline (market signal → strategy → risk → execution)
- Graceful shutdown (all agents respond to cancellation within timeout)

```bash
cargo test --workspace              # all tests
cargo test -p risk-agent            # single crate
cargo clippy --workspace            # lint (0 warnings)
```

---

## Technology Stack

| Component | Choice |
|-----------|--------|
| Language | Rust (Edition 2024) |
| Async runtime | Tokio (full features) |
| Message passing | `tokio::sync::broadcast` |
| Serialization | serde + serde_json |
| Configuration | TOML (serde-based) |
| Observability | tracing + tracing-subscriber |
| Error handling | thiserror |
| Benchmarking | Criterion |
| Shutdown | tokio-util CancellationToken |

Blockchain integration uses trait abstractions (`RpcClientTrait`, `Signer`, `Broadcaster`) — swap in production implementations (e.g., `alloy`) without changing agent logic.

---

## Production Readiness

| Layer | Status |
|-------|--------|
| Message bus & resilience | Production-ready |
| Risk validation engine | Production-ready |
| Transaction parser | Production-ready (Uniswap V2/V3) |
| Price tracking & divergence | Production-ready |
| Nonce management | Production-ready |
| Config & observability | Production-ready |
| RPC integration | Interface defined, mock implemented |
| Transaction signing | Interface defined, mock implemented |
| Transaction broadcast | Interface defined, dry-run implemented |
| Strategy plugins | Arbitrage implemented, extensible |

To connect to a real Ethereum node: implement `RpcClientTrait` with `alloy` WebSocket provider, `Signer` with `alloy-signer`, and `Broadcaster` with `alloy` transaction sending.

---

## Extending

### Add a new strategy

```rust
use async_trait::async_trait;
use common::messages::{MarketSignal, TradeIntent};
use strategy_agent::Strategy;

pub struct MyStrategy { /* config */ }

#[async_trait]
impl Strategy for MyStrategy {
    fn name(&self) -> &str { "my_strategy" }

    async fn evaluate(&self, signal: &MarketSignal) -> Option<TradeIntent> {
        // Your logic here
        None
    }
}
```

Register it in the orchestrator's strategy list.

### Add a new risk rule

Add a `check_*` function in `risk-agent/src/rules.rs` following the existing pattern, then call it from `RiskAgent::validate()`.

---

## Future Work

- Real WebSocket RPC integration (alloy)
- ECDSA transaction signing
- Live transaction broadcasting
- Liquidation and yield optimization strategies
- Persistent nonce recovery from chain
- HTTP monitoring API
- Backtesting engine with historical tx replay
- MEV bundle generation
- Cross-chain support
- Docker/CI pipeline

---

## License

MIT License
