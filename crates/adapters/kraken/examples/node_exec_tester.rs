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

//! Example demonstrating live execution testing with the Kraken adapter.
//!
//! Edit the constants below to change the product type, target symbol, and order size.
//!
//! Run with: `cargo run -p nautilus-kraken --example kraken-exec-tester --features examples`
//!
//! Required credential environment variables (Spot):
//! - `KRAKEN_SPOT_API_KEY`.
//! - `KRAKEN_SPOT_API_SECRET`.
//!
//! Required credential environment variables (Futures):
//! - `KRAKEN_FUTURES_API_KEY`.
//! - `KRAKEN_FUTURES_API_SECRET`.

use nautilus_common::enums::Environment;
use nautilus_kraken::{
    common::{consts::KRAKEN_CLIENT_ID, credential::KrakenCredential, enums::KrakenProductType},
    config::{KrakenDataClientConfig, KrakenExecClientConfig},
    factories::{KrakenDataClientFactory, KrakenExecutionClientFactory},
};
use nautilus_live::{config::LiveExecEngineConfig, node::LiveNode};
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

// *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
// *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

const PRODUCT_TYPE: KrakenProductType = KrakenProductType::Futures;
const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "KRAKEN-001";
const NODE_NAME: &str = "KRAKEN-EXEC-TESTER-001";
const STRATEGY_ID: &str = "EXEC_TESTER-001";

// Spot symbols are normalized to BTC (from Kraken's XBT).
const SPOT_SYMBOL: &str = "BTC/USD";
const SPOT_ORDER_QTY: &str = "0.0001"; // Minimum BTC quantity
// Futures perpetual symbols use the PF_ prefix (e.g. PF_XBTUSD, PF_ETHUSD).
const FUTURES_SYMBOL: &str = "PF_XBTUSD";
const FUTURES_ORDER_QTY: &str = "0.001";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let product_type = PRODUCT_TYPE;

    let (symbol, order_qty) = match product_type {
        KrakenProductType::Spot => (SPOT_SYMBOL, Quantity::from(SPOT_ORDER_QTY)),
        KrakenProductType::Futures => (FUTURES_SYMBOL, Quantity::from(FUTURES_ORDER_QTY)),
    };

    let instrument_id = InstrumentId::from(format!("{symbol}.KRAKEN").as_str());

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let node_name = NODE_NAME.to_string();
    let client_id = *KRAKEN_CLIENT_ID;

    let credential = match product_type {
        KrakenProductType::Spot => KrakenCredential::resolve_spot(None, None),
        KrakenProductType::Futures => KrakenCredential::resolve_futures(None, None, false),
    }
    .ok_or(
        "API credentials required (set KRAKEN_SPOT_API_KEY/KRAKEN_SPOT_API_SECRET \
         or KRAKEN_FUTURES_API_KEY/KRAKEN_FUTURES_API_SECRET)",
    )?;

    let (api_key, api_secret) = credential.into_parts();

    let data_config = KrakenDataClientConfig {
        api_key: Some(api_key.clone()),
        api_secret: Some(api_secret.clone()),
        product_type,
        ..Default::default()
    };

    let exec_config = KrakenExecClientConfig {
        trader_id,
        account_id,
        api_key,
        api_secret,
        product_type,
        ..Default::default()
    };

    let data_factory = KrakenDataClientFactory::new();
    let exec_factory = KrakenExecutionClientFactory::new();
    let exec_engine_config = LiveExecEngineConfig {
        open_check_interval_secs: Some(10.0),
        position_check_interval_secs: Some(30.0),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_exec_engine_config(exec_engine_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from(STRATEGY_ID)),
            external_order_claims: Some(vec![instrument_id]),
            // Kraken truncates non-UUID client order IDs to 18 chars,
            // which can cause collisions across sessions at the same time of day.
            use_uuid_client_order_ids: true,
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .use_post_only(true)
        .open_position_on_start_qty(order_qty.as_decimal())
        // .tob_offset_ticks(0)
        .log_data(false)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
