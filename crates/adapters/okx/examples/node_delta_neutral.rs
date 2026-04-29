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

//! Example demonstrating the delta-neutral volatility strategy on OKX options.
//!
//! This runs the `DeltaNeutralVol` strategy which:
//! 1. Discovers BTC option instruments from the cache
//! 2. Selects OTM call and put strikes for a short strangle
//! 3. Subscribes to option greeks and hedge instrument quotes
//! 4. Enters the strangle with IV-based limit orders (via `px_vol` param)
//! 5. Delta-hedges with the underlying inverse perpetual on a periodic timer
//!
//! OKX BTC options are inverse (coin-margined, settled in BTC). The hedge
//! instrument is the BTC-USD-SWAP inverse perpetual, so both legs share the
//! same margin currency.
//!
//! The strategy uses `px_vol` to price option orders by implied volatility,
//! which the OKX adapter translates to the `pxVol` API field.
//!
//! Configuration defaults:
//! - Target deltas: +0.20 (call), -0.20 (put)
//! - 1 contract per leg
//! - Rehedge when portfolio delta exceeds 0.5
//! - Rehedge check every 30 seconds
//! - Entry disabled by default (set `enter_strangle` to enable live orders)
//!
//! Run with: `cargo run --example okx-delta-neutral --package nautilus-okx --features examples`

use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{AccountId, ClientId, InstrumentId, TraderId};
use nautilus_okx::{
    common::enums::OKXInstrumentType,
    config::{OKXDataClientConfig, OKXExecClientConfig},
    factories::{OKXDataClientFactory, OKXExecutionClientFactory},
};
use nautilus_trading::examples::strategies::delta_neutral_vol::{
    DeltaNeutralVol, DeltaNeutralVolConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("OKX-001");
    let client_id = ClientId::new("OKX");

    let data_config = OKXDataClientConfig {
        api_key: None,        // Will use 'OKX_API_KEY' env var
        api_secret: None,     // Will use 'OKX_API_SECRET' env var
        api_passphrase: None, // Will use 'OKX_API_PASSPHRASE' env var
        instrument_types: vec![OKXInstrumentType::Option, OKXInstrumentType::Swap],
        instrument_families: Some(vec!["BTC-USD".to_string()]),
        ..Default::default()
    };

    let exec_config = OKXExecClientConfig {
        trader_id,
        account_id,
        api_key: None,        // Will use 'OKX_API_KEY' env var
        api_secret: None,     // Will use 'OKX_API_SECRET' env var
        api_passphrase: None, // Will use 'OKX_API_PASSPHRASE' env var
        instrument_types: vec![OKXInstrumentType::Option, OKXInstrumentType::Swap],
        instrument_families: Some(vec!["BTC-USD".to_string()]),
        ..Default::default()
    };

    let data_factory = OKXDataClientFactory::new();
    let exec_factory = OKXExecutionClientFactory::new();

    let hedge_instrument_id = InstrumentId::from("BTC-USD-SWAP.OKX");

    let mut strategy_config =
        DeltaNeutralVolConfig::new("BTC".to_string(), hedge_instrument_id, client_id)
            .with_contracts(1)
            .with_rehedge_delta_threshold(0.5)
            .with_rehedge_interval_secs(30)
            .with_enter_strangle(false);

    // OKX forbids hyphens in client order IDs
    strategy_config.base.use_hyphens_in_client_order_ids = false;

    let strategy = DeltaNeutralVol::new(strategy_config);

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("OKX-DELTA-NEUTRAL-001".to_string())
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
