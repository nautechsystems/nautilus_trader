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

//! Example demonstrating the delta-neutral volatility strategy on Derive options.
//!
//! This runs the `DeltaNeutralVol` strategy which:
//! 1. Discovers ETH option instruments from the cache.
//! 2. Selects OTM call and put strikes for a short strangle.
//! 3. Subscribes to option greeks and hedge instrument quotes.
//! 4. Delta-hedges with the ETH perpetual on a periodic timer.
//!
//! Edit the constants below to change the environment, option family, hedge
//! instrument, target deltas, and hedging behavior. Entry is disabled by
//! default (`ENTER_STRANGLE`).
//!
//! Run with: `cargo run --example derive-delta-neutral --package nautilus-derive --features examples`
//!
//! Required credential environment variables (testnet variants used when
//! `DERIVE_ENVIRONMENT` is `DeriveEnvironment::Testnet`):
//! - `DERIVE_WALLET_ADDRESS` / `DERIVE_TESTNET_WALLET_ADDRESS`.
//! - `DERIVE_SESSION_PRIVATE_KEY` / `DERIVE_TESTNET_SESSION_PRIVATE_KEY`.
//! - `DERIVE_SUBACCOUNT_ID` / `DERIVE_TESTNET_SUBACCOUNT_ID`.

use nautilus_common::enums::Environment;
use nautilus_derive::{
    common::{consts::DERIVE_CLIENT_ID, enums::DeriveEnvironment},
    config::{DeriveDataClientConfig, DeriveExecClientConfig},
    factories::{DeriveDataClientFactory, DeriveExecFactoryConfig, DeriveExecutionClientFactory},
};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{AccountId, InstrumentId, TraderId};
use nautilus_trading::examples::strategies::delta_neutral_vol::{
    DeltaNeutralVol, DeltaNeutralVolConfig,
};
use rust_decimal::Decimal;

const DERIVE_ENVIRONMENT: DeriveEnvironment = DeriveEnvironment::Testnet;
const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "DERIVE-001";
const NODE_NAME: &str = "DERIVE-DELTA-NEUTRAL-001";

const OPTION_FAMILY: &str = "ETH";
const HEDGE_INSTRUMENT_ID: &str = "ETH-PERP.DERIVE";

const ENTER_STRANGLE: bool = false;
const HEDGE_ENABLED: bool = true;
const REHEDGE_DELTA_THRESHOLD: f64 = 0.5;
const REHEDGE_INTERVAL_SECS: u64 = 30;

const TARGET_CALL_DELTA: f64 = 0.20;
const TARGET_PUT_DELTA: f64 = -0.20;
const CONTRACTS: u64 = 1;
const ENTRY_IV_OFFSET: f64 = 0.0;
const ENTRY_PREMIUM_OFFSET_TICKS: i32 = 1;
const EXPIRY_FILTER: Option<&str> = None;

const MAX_FEE_PER_CONTRACT: &str = "1000";
const MARKET_ORDER_SLIPPAGE_BPS: u32 = 50;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let derive_environment = DERIVE_ENVIRONMENT;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let client_id = *DERIVE_CLIENT_ID;
    let option_family = OPTION_FAMILY.to_string();
    let hedge_instrument_id = InstrumentId::from(HEDGE_INSTRUMENT_ID);

    // Disable rehedging by pushing the threshold beyond any reachable delta.
    let rehedge_delta_threshold = if HEDGE_ENABLED {
        REHEDGE_DELTA_THRESHOLD
    } else {
        1.0e12
    };

    let data_config = DeriveDataClientConfig {
        environment: derive_environment,
        currencies: vec![option_family.clone()],
        ..Default::default()
    };

    let exec_config = DeriveExecClientConfig {
        environment: derive_environment,
        max_fee_per_contract: Some(Decimal::from_str_exact(MAX_FEE_PER_CONTRACT)?),
        market_order_slippage_bps: MARKET_ORDER_SLIPPAGE_BPS,
        ..Default::default()
    };
    let exec_factory_config = DeriveExecFactoryConfig {
        trader_id,
        account_id,
        config: exec_config,
    };

    let data_factory = DeriveDataClientFactory::new();
    let exec_factory = DeriveExecutionClientFactory::new();

    let mut strategy_config = DeltaNeutralVolConfig::builder()
        .option_family(option_family)
        .hedge_instrument_id(hedge_instrument_id)
        .client_id(client_id)
        .target_call_delta(TARGET_CALL_DELTA)
        .target_put_delta(TARGET_PUT_DELTA)
        .contracts(CONTRACTS)
        .rehedge_delta_threshold(rehedge_delta_threshold)
        .rehedge_interval_secs(REHEDGE_INTERVAL_SECS)
        .enter_strangle(ENTER_STRANGLE)
        .entry_iv_offset(ENTRY_IV_OFFSET)
        .entry_premium_offset_ticks(ENTRY_PREMIUM_OFFSET_TICKS)
        .build();

    if let Some(expiry) = EXPIRY_FILTER {
        strategy_config.expiry_filter = Some(expiry.to_string());
    }

    let strategy = DeltaNeutralVol::new(strategy_config);

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(NODE_NAME.to_string())
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_factory_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
