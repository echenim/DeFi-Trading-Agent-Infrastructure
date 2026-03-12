use std::path::PathBuf;
use std::sync::Arc;

use common::config::{load_rpc_config, load_trading_config};
use execution_agent::{DryRunBroadcaster, ExecutionAgent, MockSigner, NonceManager, TransactionBuilder};
use market_data_agent::{MarketDataAgent, MockRpcClient, RpcClient};
use messaging::MessageBus;
use risk_agent::RiskAgent;
use strategy_agent::strategies::arbitrage::ArbitrageStrategy;
use strategy_agent::{Strategy, StrategyAgent};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Debug)]
struct Args {
    config_dir: PathBuf,
    dry_run: bool,
}

fn parse_args() -> Args {
    let mut config_dir = PathBuf::from("./configs");
    let mut dry_run = false;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--config-dir" => {
                i += 1;
                if i < args.len() {
                    config_dir = PathBuf::from(&args[i]);
                }
            }
            "--dry-run" => {
                dry_run = true;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    Args { config_dir, dry_run }
}

#[tokio::main]
async fn main() {
    // Parse args
    let args = parse_args();

    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(true)
        .init();

    info!(config_dir = ?args.config_dir, dry_run = args.dry_run, "starting orchestrator");

    // Load configs
    let rpc_config = match load_rpc_config(&args.config_dir) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "failed to load RPC config");
            std::process::exit(1);
        }
    };

    let trading_config = match load_trading_config(&args.config_dir) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "failed to load trading config");
            std::process::exit(1);
        }
    };

    // Override dry_run from CLI flag
    let mut execution_config = trading_config.execution.clone();
    if args.dry_run {
        execution_config.dry_run = true;
    }

    info!(?rpc_config.ethereum, "loaded RPC config");
    info!(
        min_profit_bps = trading_config.strategy.min_profit_bps,
        dry_run = execution_config.dry_run,
        "loaded trading config"
    );

    // Shared infrastructure
    let bus = MessageBus::new(trading_config.market_data.mempool_buffer_size);
    let cancel = CancellationToken::new();

    // --- Market Data Agent ---
    let market_data_handle = {
        let bus = bus.clone();
        let cancel = cancel.clone();
        let config = trading_config.market_data.clone();

        if execution_config.dry_run {
            // In dry-run mode, use mock RPC with no transactions (agent will exit immediately)
            let rpc = MockRpcClient::new(vec![]);
            tokio::spawn(async move {
                let agent = MarketDataAgent::new(rpc, bus, cancel, config);
                agent.start().await;
                info!("market data agent exited");
            })
        } else {
            let rpc = RpcClient::new(
                rpc_config.ethereum.ws_url.clone(),
                rpc_config.ethereum.http_url.clone(),
            );
            tokio::spawn(async move {
                let agent = MarketDataAgent::new(rpc, bus, cancel, config);
                agent.start().await;
                info!("market data agent exited");
            })
        }
    };

    // --- Strategy Agent ---
    let strategy_handle = {
        let bus = bus.clone();
        let cancel = cancel.clone();
        let strategies: Vec<Box<dyn Strategy>> = vec![Box::new(ArbitrageStrategy::new(
            trading_config.strategy.min_profit_bps,
            trading_config.strategy.max_trade_size_eth,
        ))];

        tokio::spawn(async move {
            let agent = StrategyAgent::new(strategies, bus, cancel);
            agent.start().await;
            info!("strategy agent exited");
        })
    };

    // --- Risk Agent ---
    let risk_handle = {
        let bus = bus.clone();
        let cancel = cancel.clone();
        let config = trading_config.risk.clone();

        tokio::spawn(async move {
            let agent = RiskAgent::new(config, bus, cancel);
            agent.start().await;
            info!("risk agent exited");
        })
    };

    // --- Execution Agent ---
    let execution_handle = {
        let bus = bus.clone();
        let cancel = cancel.clone();

        let signer: Arc<dyn execution_agent::Signer> =
            Arc::new(MockSigner::new(common::types::Address::zero()));
        let nonce_manager = Arc::new(NonceManager::new(0));
        let tx_builder = TransactionBuilder::new(rpc_config.ethereum.chain_id);
        let broadcaster: Arc<dyn execution_agent::Broadcaster> =
            Arc::new(DryRunBroadcaster::new());

        tokio::spawn(async move {
            let agent = ExecutionAgent::new(
                signer,
                nonce_manager,
                tx_builder,
                broadcaster,
                execution_config,
                bus,
                cancel,
            );
            agent.start().await;
            info!("execution agent exited");
        })
    };

    // Wait for Ctrl+C
    info!("all agents started — press Ctrl+C to shut down");

    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for Ctrl+C");

    info!("received shutdown signal, cancelling agents...");
    cancel.cancel();

    // Wait for all agents to finish
    let _ = tokio::join!(
        market_data_handle,
        strategy_handle,
        risk_handle,
        execution_handle,
    );

    info!("all agents stopped — orchestrator exiting");
}
