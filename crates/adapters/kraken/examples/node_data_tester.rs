//! Example demonstrating live data testing with the Kraken adapter.
//!
//! Run with: `cargo run -p nautilus-kraken --example kraken-data-tester`
//!
//! Environment variables (optional for public data):
//! - KRAKEN_API_KEY: Your Kraken API key
//! - KRAKEN_API_SECRET: Your Kraken API secret

use nautilus_common::enums::Environment;
use nautilus_kraken::{
    common::enums::KrakenProductType, config::KrakenDataClientConfig,
    factories::KrakenDataClientFactory,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::bar::BarType,
    identifiers::{ClientId, InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

// *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
// *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Configuration - Change product_type to switch between trading modes
    let product_type = KrakenProductType::Futures; // Spot or Futures

    // Symbol and settings based on product type
    let (symbols, subscribe_bars, subscribe_mark_prices, subscribe_index_prices) =
        match product_type {
            KrakenProductType::Spot => {
                // Spot symbols are normalized to BTC (from Kraken's XBT)
                let symbols = vec!["BTC/USD", "ETH/USD"];
                (symbols, true, false, false)
            }
            KrakenProductType::Futures => {
                // Futures perpetual symbols use PF_ prefix (e.g., PF_XBTUSD, PF_ETHUSD)
                let symbols = vec!["PF_XBTUSD", "PF_ETHUSD"];
                (symbols, false, true, true)
            }
        };

    let instrument_ids: Vec<InstrumentId> = symbols
        .iter()
        .map(|s| InstrumentId::from(format!("{s}.KRAKEN").as_str()))
        .collect();

    let bar_types: Vec<BarType> = if subscribe_bars {
        instrument_ids
            .iter()
            .map(|id| BarType::from(format!("{id}-1-MINUTE-LAST-EXTERNAL").as_str()))
            .collect()
    } else {
        vec![]
    };

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let node_name = "KRAKEN-TESTER-001".to_string();
    let client_id = ClientId::new("KRAKEN");

    let kraken_config = KrakenDataClientConfig {
        api_key: None,    // Will use 'KRAKEN_API_KEY' env var if available
        api_secret: None, // Will use 'KRAKEN_API_SECRET' env var if available
        product_type,
        ..Default::default()
    };

    let client_factory = KrakenDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(client_factory), Box::new(kraken_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let tester_config = DataTesterConfig::new(client_id, instrument_ids)
        .with_subscribe_quotes(true)
        .with_subscribe_trades(true)
        .with_bar_types(bar_types)
        .with_subscribe_bars(subscribe_bars)
        .with_subscribe_mark_prices(subscribe_mark_prices)
        .with_subscribe_index_prices(subscribe_index_prices)
        .with_request_trades(true)
        .with_request_bars(subscribe_bars)
        .with_log_data(true);

    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
