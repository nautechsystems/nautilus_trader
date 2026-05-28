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
//! Configuration defaults:
//! - Testnet is used unless `DERIVE_ENVIRONMENT=mainnet`.
//! - Target deltas: +0.20 (call), -0.20 (put).
//! - 1 contract per leg.
//! - Rehedge when portfolio delta exceeds 0.5.
//! - Rehedge check every 30 seconds.
//! - Entry disabled by default. Enable only after validating Derive option price semantics.
//!
//! Run with: `cargo run --example derive-delta-neutral --package nautilus-derive --features examples`

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let derive_environment = derive_environment_from_env();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("DERIVE-001");
    let client_id = *DERIVE_CLIENT_ID;

    let data_config = DeriveDataClientConfig {
        environment: derive_environment,
        currencies: vec!["ETH".to_string()],
        ..Default::default()
    };

    let exec_config = DeriveExecClientConfig {
        environment: derive_environment,
        max_fee_per_contract: Some(Decimal::from_str_exact("1000")?),
        domain_separator: env_override(
            derive_environment,
            "DERIVE_DOMAIN_SEPARATOR",
            "DERIVE_TESTNET_DOMAIN_SEPARATOR",
        ),
        action_typehash: env_override(
            derive_environment,
            "DERIVE_ACTION_TYPEHASH",
            "DERIVE_TESTNET_ACTION_TYPEHASH",
        ),
        trade_module_address: env_override(
            derive_environment,
            "DERIVE_TRADE_MODULE_ADDRESS",
            "DERIVE_TESTNET_TRADE_MODULE_ADDRESS",
        ),
        ..Default::default()
    };
    let exec_factory_config = DeriveExecFactoryConfig {
        trader_id,
        account_id,
        config: exec_config,
    };

    let data_factory = DeriveDataClientFactory::new();
    let exec_factory = DeriveExecutionClientFactory::new();

    let hedge_instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let strategy_config =
        DeltaNeutralVolConfig::new("ETH".to_string(), hedge_instrument_id, client_id)
            .with_contracts(1)
            .with_rehedge_delta_threshold(0.5)
            .with_rehedge_interval_secs(30)
            .with_enter_strangle(false);
    let strategy = DeltaNeutralVol::new(strategy_config);

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("DERIVE-DELTA-NEUTRAL-001".to_string())
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_factory_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

fn derive_environment_from_env() -> DeriveEnvironment {
    match std::env::var("DERIVE_ENVIRONMENT") {
        Ok(value)
            if value.eq_ignore_ascii_case("mainnet") || value.eq_ignore_ascii_case("live") =>
        {
            DeriveEnvironment::Mainnet
        }
        _ => DeriveEnvironment::Testnet,
    }
}

fn env_override(environment: DeriveEnvironment, mainnet: &str, testnet: &str) -> Option<String> {
    let var_name = if environment.is_testnet() {
        testnet
    } else {
        mainnet
    };

    std::env::var(var_name).ok()
}
