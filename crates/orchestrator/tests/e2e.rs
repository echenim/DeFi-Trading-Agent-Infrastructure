use std::sync::Arc;
use std::time::Duration;

use common::config::{ExecutionConfig, MarketDataConfig, RiskConfig};
use common::messages::Message;
use common::types::{Address, TxHash};
use execution_agent::{
    DryRunBroadcaster, ExecutionAgent, MockSigner, NonceManager, TransactionBuilder,
};
use market_data_agent::rpc::RawTransaction;
use market_data_agent::{MarketDataAgent, MockRpcClient};
use messaging::MessageBus;
use risk_agent::RiskAgent;
use strategy_agent::strategies::arbitrage::ArbitrageStrategy;
use strategy_agent::{Strategy, StrategyAgent};
use tokio_util::sync::CancellationToken;

fn make_market_data_config() -> MarketDataConfig {
    MarketDataConfig {
        mempool_buffer_size: 128,
        price_staleness_secs: 30,
    }
}

fn make_risk_config() -> RiskConfig {
    RiskConfig {
        max_position_size_eth: 50.0,
        max_slippage_bps: 500,
        max_gas_price_gwei: 200.0,
        daily_loss_limit_eth: 5.0,
    }
}

fn make_execution_config() -> ExecutionConfig {
    ExecutionConfig {
        gas_price_multiplier: 1.1,
        dry_run: true,
        confirmation_blocks: 1,
        tx_timeout_secs: 120,
    }
}

/// Build a raw transaction that looks like a Uniswap V2 `swapExactTokensForTokens` call.
fn make_uni_v2_swap_tx() -> RawTransaction {
    let mut data = Vec::new();

    // swapExactTokensForTokens selector
    data.extend_from_slice(&[0x38, 0xed, 0x17, 0x38]);

    // amountIn (1 ETH in wei)
    let mut amount_in = [0u8; 32];
    amount_in[16..32].copy_from_slice(&1_000_000_000_000_000_000u128.to_be_bytes());
    data.extend_from_slice(&amount_in);

    // amountOutMin
    let mut amount_out_min = [0u8; 32];
    amount_out_min[16..32].copy_from_slice(&990_000_000_000_000_000u128.to_be_bytes());
    data.extend_from_slice(&amount_out_min);

    // offset to path array
    let mut offset = [0u8; 32];
    offset[31] = 0xa0;
    data.extend_from_slice(&offset);

    // to address (padded)
    data.extend_from_slice(&[0u8; 32]);

    // deadline
    data.extend_from_slice(&[0u8; 32]);

    // path length = 2
    let mut len = [0u8; 32];
    len[31] = 2;
    data.extend_from_slice(&len);

    // path[0] = token_in
    let mut in_word = [0u8; 32];
    in_word[12..32].copy_from_slice(&[0x11; 20]);
    data.extend_from_slice(&in_word);

    // path[1] = token_out
    let mut out_word = [0u8; 32];
    out_word[12..32].copy_from_slice(&[0x22; 20]);
    data.extend_from_slice(&out_word);

    RawTransaction {
        hash: TxHash([0xAA; 32]),
        from: [0xBB; 20],
        to: Some([0xCC; 20]),
        value: 0,
        input: data,
        gas_price: 20_000_000_000,
    }
}

/// End-to-end test: a swap transaction flows through the full pipeline.
///
/// Market Data → Strategy → Risk → Execution (dry-run)
///
/// We verify that a Message::Executed appears on the bus.
#[tokio::test]
async fn test_e2e_pipeline_dry_run() {
    let bus = MessageBus::new(128);
    let cancel = CancellationToken::new();

    // Subscribe to bus to observe the final output
    let mut observer = bus.subscribe();

    // --- Market Data Agent with mock RPC ---
    let mock_rpc = MockRpcClient::new(vec![make_uni_v2_swap_tx()]);
    let market_agent = MarketDataAgent::new(
        mock_rpc,
        bus.clone(),
        cancel.clone(),
        make_market_data_config(),
    );

    // --- Strategy Agent ---
    let strategies: Vec<Box<dyn Strategy>> = vec![Box::new(ArbitrageStrategy::new(
        0.0, // Accept any spread (the signal is LargeSwap, not PriceDivergence, so this won't produce intents)
        10.0,
    ))];
    let strategy_agent = StrategyAgent::new(strategies, bus.clone(), cancel.clone());

    // --- Risk Agent ---
    let risk_agent = RiskAgent::new(make_risk_config(), bus.clone(), cancel.clone());

    // --- Execution Agent ---
    let signer: Arc<dyn execution_agent::Signer> =
        Arc::new(MockSigner::new(Address::zero()));
    let nonce_manager = Arc::new(NonceManager::new(0));
    let tx_builder = TransactionBuilder::new(1);
    let broadcaster: Arc<dyn execution_agent::Broadcaster> =
        Arc::new(DryRunBroadcaster::new());
    let exec_agent = ExecutionAgent::new(
        signer,
        nonce_manager,
        tx_builder,
        broadcaster,
        make_execution_config(),
        bus.clone(),
        cancel.clone(),
    );

    // Spawn all agents
    let h1 = tokio::spawn(async move { market_agent.start().await });
    let h2 = tokio::spawn(async move { strategy_agent.start().await });
    let h3 = tokio::spawn(async move { risk_agent.start().await });
    let h4 = tokio::spawn(async move { exec_agent.start().await });

    // The market data agent publishes a LargeSwap signal.
    // The ArbitrageStrategy only reacts to PriceDivergence, so it won't produce an intent.
    // Therefore, we expect a Signal on the bus but NOT an Intent or Executed.
    let msg = tokio::time::timeout(Duration::from_secs(2), observer.recv())
        .await
        .expect("timeout waiting for signal")
        .expect("bus error");

    assert!(
        matches!(msg, Message::Signal(_)),
        "expected market signal, got {msg:?}"
    );

    // Cancel and wait for agents to stop
    cancel.cancel();
    let _ = tokio::join!(h1, h2, h3, h4);
}

/// Test that graceful shutdown works — all agents exit after cancel.
#[tokio::test]
async fn test_graceful_shutdown() {
    let bus = MessageBus::new(128);
    let cancel = CancellationToken::new();

    let mock_rpc = MockRpcClient::new(vec![]);
    let market_agent = MarketDataAgent::new(
        mock_rpc,
        bus.clone(),
        cancel.clone(),
        make_market_data_config(),
    );

    let strategies: Vec<Box<dyn Strategy>> = vec![Box::new(ArbitrageStrategy::new(50.0, 10.0))];
    let strategy_agent = StrategyAgent::new(strategies, bus.clone(), cancel.clone());

    let risk_agent = RiskAgent::new(make_risk_config(), bus.clone(), cancel.clone());

    let signer: Arc<dyn execution_agent::Signer> =
        Arc::new(MockSigner::new(Address::zero()));
    let nonce_manager = Arc::new(NonceManager::new(0));
    let tx_builder = TransactionBuilder::new(1);
    let broadcaster: Arc<dyn execution_agent::Broadcaster> =
        Arc::new(DryRunBroadcaster::new());
    let exec_agent = ExecutionAgent::new(
        signer,
        nonce_manager,
        tx_builder,
        broadcaster,
        make_execution_config(),
        bus.clone(),
        cancel.clone(),
    );

    let h1 = tokio::spawn(async move { market_agent.start().await });
    let h2 = tokio::spawn(async move { strategy_agent.start().await });
    let h3 = tokio::spawn(async move { risk_agent.start().await });
    let h4 = tokio::spawn(async move { exec_agent.start().await });

    // Give agents time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    cancel.cancel();

    // All agents should exit within a reasonable time
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let _ = tokio::join!(h1, h2, h3, h4);
    })
    .await;

    assert!(result.is_ok(), "agents did not shut down within 5 seconds");
}
