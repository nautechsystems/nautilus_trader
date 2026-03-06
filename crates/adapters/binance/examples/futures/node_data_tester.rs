//! Example demonstrating live data testing with the Binance Futures USD-M adapter.
//!
//! Run with: `cargo run --example binance-futures-data-tester --package nautilus-binance`
//!
//! Uses testnet by default for safety.

use std::num::NonZeroUsize;

use nautilus_binance::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    config::BinanceDataClientConfig,
    factories::BinanceDataClientFactory,
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
    let node_name = "BINANCE-FUTURES-TESTER-001".to_string();
    let instrument_ids = vec![
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        // InstrumentId::from("ETHUSDT-PERP.BINANCE"),
    ];

    let binance_config = BinanceDataClientConfig {
        product_types: vec![BinanceProductType::UsdM],
        environment: BinanceEnvironment::Testnet,
        api_key: None,
        api_secret: None,
        ..Default::default()
    };

    let client_factory = BinanceDataClientFactory::new();
    let client_id = ClientId::new("BINANCE");

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(binance_config))?
        .build()?;

    let tester_config = DataTesterConfig::new(client_id, instrument_ids)
        .with_subscribe_book_at_interval(true)
        .with_book_depth(NonZeroUsize::new(20))
        .with_book_interval_ms(NonZeroUsize::new(10).unwrap());
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
