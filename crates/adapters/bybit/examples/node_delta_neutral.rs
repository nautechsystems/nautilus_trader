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
//! Configuration defaults:
//! - Target deltas: +0.20 (call), -0.20 (put)
//! - 1 contract per leg
//! - Rehedge when portfolio delta exceeds 0.5
//! - Rehedge check every 30 seconds
//! - Entry disabled by default (set `enter_strangle` to enable live orders)
//!
//! Run with: `cargo run --example bybit-delta-neutral --package nautilus-bybit --features examples`

use nautilus_bybit::{
    common::enums::BybitProductType,
    config::{BybitDataClientConfig, BybitExecClientConfig},
    factories::{BybitDataClientFactory, BybitExecutionClientFactory},
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{AccountId, ClientId, InstrumentId, TraderId};
use nautilus_trading::examples::strategies::delta_neutral_vol::{
    DeltaNeutralVol, DeltaNeutralVolConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = ClientId::new("BYBIT");

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

    let hedge_instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");

    let strategy_config =
        DeltaNeutralVolConfig::new("BTC".to_string(), hedge_instrument_id, client_id)
            .with_contracts(1)
            .with_rehedge_delta_threshold(0.5)
            .with_rehedge_interval_secs(30)
            .with_enter_strangle(false)
            .with_iv_param_key("order_iv".to_string());

    let strategy = DeltaNeutralVol::new(strategy_config);

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("BYBIT-DELTA-NEUTRAL-001".to_string())
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
