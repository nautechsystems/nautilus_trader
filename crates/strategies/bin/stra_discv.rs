//! Run the address discovery strategy on Hyperliquid mainnet.
//!
//! Subscribes to trades for every supported instrument and discovers participant addresses.
//! Discovered participants are persisted to PostgreSQL and periodically enriched with profiles.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-strategies --bin stra-discv
//! ```
//!
//! Environment variables:
//! - `HYPERLIQUID_MAINNET_PK` (optional, for authenticated endpoints)
//! - `POSTGRES_HOST` (default: `localhost`)
//! - `POSTGRES_PORT` (default: `5432`)
//! - `POSTGRES_USERNAME` (default: `nautilus`)
//! - `POSTGRES_PASSWORD` (default: empty)
//! - `POSTGRES_DATABASE` (default: `nautilus`)

use std::error::Error;

use nautilus_common::{
    cache::database::CacheDatabaseFactory, enums::Environment, logging::logger::LoggerConfig,
};
use nautilus_hyperliquid::{
    common::enums::HyperliquidEnvironment, config::HyperliquidDataClientConfig,
    factories::HyperliquidDataClientFactory,
};
use nautilus_infrastructure::sql::cache::PostgresCacheConfig;
use nautilus_live::node::{
    LiveNode,
    config::{LiveDataEngineConfig, LiveExecEngineConfig},
};
use nautilus_model::identifiers::TraderId;
use nautilus_strategies::discv::{AddrDiscovery, AddrDiscoveryConfig};

const TRADER_ID: &str = "stra-discv";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let trader_id = TraderId::from(TRADER_ID);

    let data_config = HyperliquidDataClientConfig::builder()
        .environment(HyperliquidEnvironment::Mainnet)
        .build();

    let data_engine_config = LiveDataEngineConfig::builder()
        .profile_refresh_cooldown_ms(5_000) // 5s cooldown between refresh cycles
        .profile_refresh_batch_size(50)
        .build();

    // Read NAUTILUS_LOG env var for logging config (e.g. "stdout=Debug;print_config")
    let logging = LoggerConfig::from_env().unwrap_or_default();

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(TRADER_ID.to_string())
        .with_logging(logging)
        .with_reconciliation(false)
        .with_data_engine_config(data_engine_config)
        .with_exec_engine_config(LiveExecEngineConfig::builder().load_cache(false).build())
        .add_data_client(
            None,
            Box::new(HyperliquidDataClientFactory::new()),
            Box::new(data_config),
        )?
        .build()?;

    // Connect PostgreSQL cache database for participant persistence
    let pg_config = PostgresCacheConfig {
        host: std::env::var("POSTGRES_HOST").ok(),
        port: std::env::var("POSTGRES_PORT")
            .ok()
            .and_then(|s| s.parse().ok()),
        username: std::env::var("POSTGRES_USERNAME").ok(),
        password: std::env::var("POSTGRES_PASSWORD").ok(),
        database: Some(
            std::env::var("POSTGRES_DATABASE").unwrap_or_else(|_| "nautilus".to_string()),
        ),
    };
    let cache_db = pg_config
        .create(trader_id, nautilus_core::UUID4::new(), Default::default())
        .await?;
    node.set_cache_database(cache_db)?;

    let strategy_config = AddrDiscoveryConfig::builder().build();
    node.add_strategy(AddrDiscovery::from_config(strategy_config))?;

    log::debug!("Starting address discovery on all Hyperliquid mainnet instruments...");
    log::debug!("Participant persistence: PostgreSQL");
    log::debug!("Profile refresh: every 30s, batch size 50");
    log::debug!("Press Ctrl+C to stop.");

    node.run().await?;

    Ok(())
}
