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
//! - `DERIVE_DELTA_NEUTRAL_OPTION_FAMILY=ETH`.
//! - `DERIVE_DELTA_NEUTRAL_HEDGE_INSTRUMENT=ETH-PERP.DERIVE`.
//! - Target deltas: +0.20 (call), -0.20 (put).
//! - 1 contract per leg.
//! - Rehedge when portfolio delta exceeds 0.5.
//! - Rehedge check every 30 seconds.
//! - Entry disabled by default. When enabled, option entry uses live ask premium plus 1 tick.
//!
//! Run with: `cargo run --example derive-delta-neutral --package nautilus-derive --features examples`

use std::{env, error::Error, io};

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
    let option_family = env_string("DERIVE_DELTA_NEUTRAL_OPTION_FAMILY", "ETH")?;
    let default_hedge = format!("{option_family}-PERP.DERIVE");
    let hedge_instrument = env_string("DERIVE_DELTA_NEUTRAL_HEDGE_INSTRUMENT", &default_hedge)?;
    let enter_strangle = env_bool("DERIVE_DELTA_NEUTRAL_ENTER_STRANGLE", false)?;
    let hedge_enabled = env_bool("DERIVE_DELTA_NEUTRAL_HEDGE_ENABLED", true)?;
    let rehedge_delta_threshold = if hedge_enabled {
        env_f64("DERIVE_DELTA_NEUTRAL_REHEDGE_DELTA_THRESHOLD", 0.5)?
    } else {
        1.0e12
    };
    let rehedge_interval_secs = env_u64("DERIVE_DELTA_NEUTRAL_REHEDGE_INTERVAL_SECS", 30)?;

    let data_config = DeriveDataClientConfig {
        environment: derive_environment,
        currencies: vec![option_family.clone()],
        ..Default::default()
    };

    let exec_config = DeriveExecClientConfig {
        environment: derive_environment,
        max_fee_per_contract: Some(env_decimal(
            "DERIVE_DELTA_NEUTRAL_MAX_FEE_PER_CONTRACT",
            Decimal::from_str_exact("1000")?,
        )?),
        market_order_slippage_bps: env_u32(
            "DERIVE_DELTA_NEUTRAL_MARKET_ORDER_SLIPPAGE_BPS",
            DeriveExecClientConfig::default().market_order_slippage_bps,
        )?,
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

    let hedge_instrument_id = InstrumentId::from(hedge_instrument.as_str());
    let mut strategy_config =
        DeltaNeutralVolConfig::new(option_family, hedge_instrument_id, client_id)
            .with_target_call_delta(env_f64("DERIVE_DELTA_NEUTRAL_TARGET_CALL_DELTA", 0.20)?)
            .with_target_put_delta(env_f64("DERIVE_DELTA_NEUTRAL_TARGET_PUT_DELTA", -0.20)?)
            .with_contracts(env_u64("DERIVE_DELTA_NEUTRAL_CONTRACTS", 1)?)
            .with_rehedge_delta_threshold(rehedge_delta_threshold)
            .with_rehedge_interval_secs(rehedge_interval_secs)
            .with_enter_strangle(enter_strangle)
            .with_entry_iv_offset(env_f64("DERIVE_DELTA_NEUTRAL_ENTRY_IV_OFFSET", 0.0)?)
            .with_entry_premium_offset_ticks(env_i32(
                "DERIVE_DELTA_NEUTRAL_ENTRY_PREMIUM_OFFSET_TICKS",
                1,
            )?);

    if let Some(expiry) = env_optional_string("DERIVE_DELTA_NEUTRAL_EXPIRY")? {
        strategy_config = strategy_config.with_expiry_filter(expiry);
    }

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
    match env::var("DERIVE_ENVIRONMENT") {
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

    env::var(var_name).ok()
}

fn env_string(name: &str, default: &str) -> Result<String, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) => Err(invalid_input_error(format!("{name} must not be empty"))),
        Err(env::VarError::NotPresent) => Ok(default.to_string()),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn env_optional_string(name: &str) -> Result<Option<String>, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Err(invalid_input_error(format!("{name} must not be empty"))),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(invalid_input_error(format!(
                "{name} must be one of true, false, 1, 0, yes, no, on, off; received {value}",
            ))),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => value.parse::<u64>().map_err(|e| {
            invalid_input_error(format!(
                "{name} must be an unsigned integer; received {value}: {e}"
            ))
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn env_u32(name: &str, default: u32) -> Result<u32, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => value.parse::<u32>().map_err(|e| {
            invalid_input_error(format!(
                "{name} must be an unsigned integer; received {value}: {e}"
            ))
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn env_i32(name: &str, default: i32) -> Result<i32, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => value.parse::<i32>().map_err(|e| {
            invalid_input_error(format!("{name} must be an integer; received {value}: {e}"))
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn env_f64(name: &str, default: f64) -> Result<f64, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => value.parse::<f64>().map_err(|e| {
            invalid_input_error(format!("{name} must be a number; received {value}: {e}"))
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn env_decimal(name: &str, default: Decimal) -> Result<Decimal, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => Decimal::from_str_exact(value.as_str()).map_err(|e| {
            invalid_input_error(format!("{name} must be a decimal; received {value}: {e}"))
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(invalid_input_error(format!("failed to read {name}: {e}"))),
    }
}

fn invalid_input_error(message: String) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidInput, message).into()
}
