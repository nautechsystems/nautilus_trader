//! Example demonstrating live data testing with the Bybit adapter.
//!
//! Run with: `cargo run --example bybit-data-tester --package nautilus-bybit`

use nautilus_bybit::{
    common::enums::BybitProductType, config::BybitDataClientConfig,
    factories::BybitDataClientFactory,
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let node_name = "BYBIT-TESTER-001".to_string();
    let instrument_ids = vec![
        InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
        // InstrumentId::from("ETHUSDT-LINEAR.BYBIT"),
    ];

    let bybit_config = BybitDataClientConfig {
        api_key: None,    // Will use 'BYBIT_API_KEY' env var
        api_secret: None, // Will use 'BYBIT_API_SECRET' env var
        product_types: vec![BybitProductType::Linear],
        ..Default::default()
    };

    let client_factory = BybitDataClientFactory::new();
    let client_id = ClientId::new("BYBIT");

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(bybit_config))?
        .build()?;

    let tester_config = DataTesterConfig::new(client_id, instrument_ids)
        .with_subscribe_quotes(true)
        .with_subscribe_trades(true)
        .with_subscribe_mark_prices(true)
        .with_subscribe_index_prices(true)
        .with_subscribe_funding_rates(true);
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
