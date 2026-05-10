// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Example demonstrating live execution testing with the Coinbase adapter on a
//! spot product (`BTC-USD`).
//!
//! Run with: `cargo run --example coinbase-exec-tester --package nautilus-coinbase --features examples`
//!
//! Required environment variables:
//! - `COINBASE_API_KEY`: CDP API key name (`organizations/{org_id}/apiKeys/{key_id}`)
//! - `COINBASE_API_SECRET`: PEM-encoded EC private key (ECDSA, not Ed25519)
//!
//! The CDP key must have View + Trade permissions. See the integration guide
//! for setup details.

use ahash::AHashMap;
use log::LevelFilter;
use nautilus_coinbase::{
    config::{CoinbaseDataClientConfig, CoinbaseExecClientConfig},
    factories::{CoinbaseDataClientFactory, CoinbaseExecutionClientFactory},
};
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_live::{config::LiveExecEngineConfig, node::LiveNode};
use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;
use ustr::Ustr;

// *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
// *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("COINBASE-001");
    let node_name = "COINBASE-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("COINBASE");
    // Pick the product whose quote currency matches a funded wallet in the
    // bound portfolio. USD and USDC are separate accounts on Coinbase, and
    // `POST /orders` rejects with "account is not available" if the quote
    // currency wallet does not exist or is empty.
    let instrument_id = InstrumentId::from("BTC-USDC.COINBASE");

    let data_config = CoinbaseDataClientConfig {
        api_key: None,    // Will use 'COINBASE_API_KEY' env var
        api_secret: None, // Will use 'COINBASE_API_SECRET' env var
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = CoinbaseExecClientConfig {
        api_key: None,    // Will use 'COINBASE_API_KEY' env var
        api_secret: None, // Will use 'COINBASE_API_SECRET' env var
        // Cash dispatches the spot bootstrap and uses the /accounts endpoint;
        // set to AccountType::Margin for CFM derivatives.
        account_type: AccountType::Cash,
        // Required when the CDP key is bound to a non-default portfolio,
        // otherwise Coinbase rejects orders with "account is not available".
        // Look up via GET /api/v3/brokerage/portfolios.
        retail_portfolio_id: None,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let data_factory = CoinbaseDataClientFactory::new();
    let exec_factory = CoinbaseExecutionClientFactory::new(trader_id, account_id);

    // The user-channel handler enriches missing `price` / `stop_price` /
    // `trigger_type` fields from REST on first sight, so external LIMIT and
    // STOP_LIMIT orders are safe under the default
    // `filter_unclaimed_external_orders=false`.
    let exec_engine_config = LiveExecEngineConfig::builder().build();

    // Coinbase modules at debug surface subscribe / parse details for new
    // deployments; switch to Info once the feed is trusted.
    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        module_level: AHashMap::from_iter([(Ustr::from("nautilus_coinbase"), LevelFilter::Debug)]),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_load_state(false)
        .with_save_state(false)
        .with_logging(log_config)
        .with_exec_engine_config(exec_engine_config)
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .build()?;

    let order_qty = Quantity::from("0.0001");
    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC_TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .tob_offset_ticks(500)
        .use_post_only(true)
        .enable_limit_buys(true)
        .enable_limit_sells(true)
        .enable_stop_buys(false)
        .enable_stop_sells(false)
        .cancel_orders_on_stop(true)
        .close_positions_on_stop(true)
        .reduce_only_on_stop(false)
        .log_data(false)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
