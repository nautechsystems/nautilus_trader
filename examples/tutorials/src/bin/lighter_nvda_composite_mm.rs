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

//! Example demonstrating Lighter NVDA RWA market making with a Databento signal.
//!
//! This builds a live node with:
//! - Databento `NVDA.EQUS` quotes as the signal instrument.
//! - Lighter `NVDA-PERP.LIGHTER` data and execution as the target instrument.
//! - The native Rust `CompositeMarketMaker` strategy.
//!
//! The default path builds the node and exits without connecting. Edit
//! `RUN_LIVE` and `ALLOW_LIVE_ORDERS` below to connect and allow live post-only
//! order submission.
//!
//! Run with:
//! `cargo run --bin lighter-nvda-composite-mm --package nautilus-tutorials --features examples`
//!
//! Required credential environment variables:
//! - `DATABENTO_API_KEY`.
//! - `LIGHTER_TESTNET_ACCOUNT_INDEX`, `LIGHTER_TESTNET_API_KEY_INDEX`, and
//!   `LIGHTER_TESTNET_API_SECRET` when the `LIGHTER_ENVIRONMENT` source constant
//!   is `LighterEnvironment::Testnet`.
//! - `LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX`, and `LIGHTER_API_SECRET`
//!   when the `LIGHTER_ENVIRONMENT` source constant is
//!   `LighterEnvironment::Mainnet`.

use std::{error::Error, io, path::PathBuf, str::FromStr};

use nautilus_common::enums::Environment;
use nautilus_core::env::get_env_var;
use nautilus_databento::factories::{DatabentoDataClientFactory, DatabentoLiveClientConfig};
use nautilus_lighter::{
    common::enums::LighterEnvironment,
    config::{LighterDataClientConfig, LighterExecClientConfig},
    factories::{LighterDataClientFactory, LighterExecutionClientFactory},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_trading::examples::strategies::composite_market_maker::{
    CompositeMarketMaker, CompositeMarketMakerConfig,
};

const RUN_LIVE: bool = false;
const ALLOW_LIVE_ORDERS: bool = false;
const LIGHTER_ENVIRONMENT: LighterEnvironment = LighterEnvironment::Testnet;

const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "LIGHTER-001";
const INSTRUMENT_ID: &str = "NVDA-PERP.LIGHTER";
const SIGNAL_INSTRUMENT_ID: &str = "NVDA.EQUS";

const MAX_POSITION: &str = "0.20";
const TRADE_SIZE: &str = "0.05";
const HALF_SPREAD_BPS: u32 = 25;
const INVENTORY_SKEW_FACTOR: f64 = 2.0;
const SIGNAL_SKEW_FACTOR: f64 = 55.0;
const REQUOTE_THRESHOLD_BPS: u32 = 5;
const ON_CANCEL_RESUBMIT: bool = false;
const SIGNAL_BASELINE: Option<f64> = None;
const EXPIRE_TIME_SECS: Option<u64> = None;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    if RUN_LIVE && !ALLOW_LIVE_ORDERS {
        return Err(invalid_input_error(
            "set ALLOW_LIVE_ORDERS to true before running live".to_string(),
        ));
    }

    let environment = Environment::Live;
    let lighter_environment = LIGHTER_ENVIRONMENT;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let signal_instrument_id = InstrumentId::from(SIGNAL_INSTRUMENT_ID);

    let databento_api_key = get_env_var("DATABENTO_API_KEY")?;
    if databento_api_key.trim().is_empty() {
        return Err(invalid_input_error(
            "DATABENTO_API_KEY must not be empty".to_string(),
        ));
    }

    let publishers_filepath = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/adapters/databento/publishers.json");
    let databento_config =
        DatabentoLiveClientConfig::new(databento_api_key, publishers_filepath, true, true);
    let lighter_data_config = LighterDataClientConfig {
        environment: lighter_environment,
        ..Default::default()
    };
    let lighter_exec_config = LighterExecClientConfig::builder()
        .trader_id(trader_id)
        .account_id(account_id)
        .environment(lighter_environment)
        .build();

    let max_position = parse_quantity("MAX_POSITION", MAX_POSITION)?;
    let trade_size = parse_quantity("TRADE_SIZE", TRADE_SIZE)?;

    let strategy_config =
        CompositeMarketMakerConfig::new(instrument_id, signal_instrument_id, max_position)
            .with_strategy_id(StrategyId::from("NVDA_COMPOSITE_MM-001"))
            .with_order_id_tag("001".to_string())
            .with_trade_size(trade_size)
            .with_half_spread_bps(HALF_SPREAD_BPS)
            .with_inventory_skew_factor(INVENTORY_SKEW_FACTOR)
            .with_signal_skew_factor(SIGNAL_SKEW_FACTOR)
            .with_requote_threshold_bps(REQUOTE_THRESHOLD_BPS)
            .with_on_cancel_resubmit(ON_CANCEL_RESUBMIT);
    let strategy_config = match SIGNAL_BASELINE {
        Some(baseline) => strategy_config.with_signal_baseline(baseline),
        None => strategy_config,
    };
    let strategy_config = match EXPIRE_TIME_SECS {
        Some(secs) => strategy_config.with_expire_time_secs(secs),
        None => strategy_config,
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("LIGHTER-NVDA-COMPOSITE-MM-001".to_string())
        .with_reconciliation(RUN_LIVE)
        .with_delay_post_stop_secs(5)
        .add_data_client(
            None,
            Box::new(DatabentoDataClientFactory::new()),
            Box::new(databento_config),
        )?
        .add_data_client(
            None,
            Box::new(LighterDataClientFactory::new()),
            Box::new(lighter_data_config),
        )?
        .add_exec_client(
            None,
            Box::new(LighterExecutionClientFactory::new()),
            Box::new(lighter_exec_config),
        )?
        .build()?;

    node.add_strategy(CompositeMarketMaker::new(strategy_config))?;

    if RUN_LIVE {
        node.run().await?;
    } else {
        println!(
            "Built Lighter NVDA composite market maker node. \
             Set RUN_LIVE and ALLOW_LIVE_ORDERS to true in this file to connect."
        );
    }

    Ok(())
}

fn parse_quantity(name: &str, value: &str) -> Result<Quantity, Box<dyn Error>> {
    Quantity::from_str(value).map_err(|e| {
        invalid_input_error(format!("{name} must be a quantity; received {value}: {e}"))
    })
}

fn invalid_input_error(message: String) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidInput, message).into()
}
