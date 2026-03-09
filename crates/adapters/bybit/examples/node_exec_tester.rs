//! Example demonstrating live execution testing with the Bybit adapter.
//!
//! Run with: `cargo run --example bybit-exec-tester --package nautilus-bybit`

use nautilus_bybit::{
    common::enums::BybitProductType,
    config::{BybitDataClientConfig, BybitExecClientConfig},
    factories::{BybitDataClientFactory, BybitExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let node_name = "BYBIT-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("BYBIT");
    let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");

    let data_config = BybitDataClientConfig {
        api_key: None,    // Will use 'BYBIT_API_KEY' env var
        api_secret: None, // Will use 'BYBIT_API_SECRET' env var
        product_types: vec![BybitProductType::Spot, BybitProductType::Linear],
        ..Default::default()
    };

    let exec_config = BybitExecClientConfig {
        api_key: None,    // Will use 'BYBIT_API_KEY' env var
        api_secret: None, // Will use 'BYBIT_API_SECRET' env var
        product_types: vec![BybitProductType::Spot, BybitProductType::Linear],
        account_id: Some(account_id),
        ..Default::default()
    };

    let data_factory = BybitDataClientFactory::new();
    let exec_factory = BybitExecutionClientFactory::new(trader_id, account_id);

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("EXEC_TESTER-001"),
        instrument_id,
        client_id,
        Quantity::from("0.01"),
    )
    .with_log_data(false)
    .with_use_post_only(true)
    .with_cancel_orders_on_stop(true)
    .with_close_positions_on_stop(true);

    tester_config.base.external_order_claims = Some(vec![instrument_id]);

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
