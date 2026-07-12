//! Run the address discovery strategy on Hyperliquid testnet.
//!
//! Subscribes to trades for ETH-PERP and logs every TradeTick received,
//! so you can inspect what fields are actually available at runtime.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-strategies --bin stra-discv
//! ```
//!
//! Environment variables:
//! - `HYPERLIQUID_TESTNET_PK` (optional, for authenticated endpoints)

use std::error::Error;

use nautilus_common::enums::Environment;
use nautilus_hyperliquid::{
    common::enums::HyperliquidEnvironment, config::HyperliquidDataClientConfig,
    factories::HyperliquidDataClientFactory,
};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{InstrumentId, TraderId};
use nautilus_strategies::discv::{AddrDiscovery, AddrDiscoveryConfig};

const TRADER_ID: &str = "ADDR-DISCV-001";
const INSTRUMENT: &str = "ETH-USD-PERP.HYPERLIQUID";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let trader_id = TraderId::from(TRADER_ID);
    let instrument_id = InstrumentId::from(INSTRUMENT);

    let data_config = HyperliquidDataClientConfig::builder()
        .environment(HyperliquidEnvironment::Testnet)
        .build();

    let strategy_config = AddrDiscoveryConfig::builder()
        .instrument_ids(vec![instrument_id])
        .build();

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(TRADER_ID.to_string())
        .with_reconciliation(false)
        .add_data_client(
            None,
            Box::new(HyperliquidDataClientFactory::new()),
            Box::new(data_config),
        )?
        .build()?;

    node.add_strategy(AddrDiscovery::from_config(strategy_config))?;

    println!("Starting address discovery on {INSTRUMENT} (testnet)...");
    println!("Press Ctrl+C to stop.");

    node.run().await?;

    Ok(())
}
