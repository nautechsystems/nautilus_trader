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

//! Example demonstrating the delta-neutral volatility strategy on Bybit options.
//!
//! This runs the `DeltaNeutralVol` strategy which:
//! 1. Discovers BTC option instruments from the cache
//! 2. Selects OTM call and put strikes for a short strangle
//! 3. Subscribes to option greeks and hedge instrument quotes
//! 4. Enters the strangle with IV-based limit orders (via `order_iv` param)
//! 5. Delta-hedges with the underlying perpetual on a periodic timer
//!
//! The strategy uses `order_iv` to price option orders by implied volatility,
//! which the Bybit adapter translates to the `orderIv` API field.
//!
//! Edit the constants below to change the option family, hedge instrument, target
//! deltas, and hedging behavior. Entry is disabled by default (`ENTER_STRANGLE`).
//!
//! Run with: `cargo run --example bybit-delta-neutral --package nautilus-bybit --features examples`
//!
//! Credentials are read from the environment when set:
//! - `BYBIT_API_KEY`.
//! - `BYBIT_API_SECRET`.

use nautilus_bybit::{
    common::{consts::BYBIT_CLIENT_ID, enums::BybitProductType},
    config::{BybitDataClientConfig, BybitExecClientConfig},
    factories::{BybitDataClientFactory, BybitExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{AccountId, InstrumentId, TraderId};
use nautilus_trading::examples::strategies::delta_neutral_vol::{
    DeltaNeutralVol, DeltaNeutralVolConfig,
};

const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "BYBIT-001";
const NODE_NAME: &str = "BYBIT-DELTA-NEUTRAL-001";

const OPTION_FAMILY: &str = "BTC";
const HEDGE_INSTRUMENT_ID: &str = "BTCUSDT-LINEAR.BYBIT";

const CONTRACTS: u64 = 1;
const REHEDGE_DELTA_THRESHOLD: f64 = 0.5;
const REHEDGE_INTERVAL_SECS: u64 = 30;
const ENTER_STRANGLE: bool = false;
const IV_PARAM_KEY: &str = "order_iv";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let client_id = *BYBIT_CLIENT_ID;

    let data_config = BybitDataClientConfig {
        api_key: None,
        api_secret: None,
        product_types: vec![BybitProductType::Option, BybitProductType::Linear],
        ..Default::default()
    };

    let exec_config = BybitExecClientConfig {
        api_key: None,
        api_secret: None,
        product_types: vec![BybitProductType::Option, BybitProductType::Linear],
        account_id: Some(account_id),
        ..Default::default()
    };

    let data_factory = BybitDataClientFactory::new();
    let exec_factory = BybitExecutionClientFactory::new(trader_id, account_id);

    let hedge_instrument_id = InstrumentId::from(HEDGE_INSTRUMENT_ID);

    let strategy_config =
        DeltaNeutralVolConfig::new(OPTION_FAMILY.to_string(), hedge_instrument_id, client_id)
            .with_contracts(CONTRACTS)
            .with_rehedge_delta_threshold(REHEDGE_DELTA_THRESHOLD)
            .with_rehedge_interval_secs(REHEDGE_INTERVAL_SECS)
            .with_enter_strangle(ENTER_STRANGLE)
            .with_iv_param_key(IV_PARAM_KEY.to_string());

    let strategy = DeltaNeutralVol::new(strategy_config);

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(NODE_NAME.to_string())
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
