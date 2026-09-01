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

//! Live execution tester targeting a HIP-4 outcome side token.
//!
//! Exercises a Yes-side BTC daily market from the Rust live node.
//!
//! Edit the environment constant below and set the outcome instrument and order size through the
//! required environment variables.
//!
//! Run with:
//! `cargo run --example hyperliquid-outcome-exec-tester --package nautilus-hyperliquid --features examples`
//!
//! Required credential environment variables:
//! - `HYPERLIQUID_PK` (mainnet) or `HYPERLIQUID_TESTNET_PK` (testnet)
//! - Optionally `HYPERLIQUID_ACCOUNT_ADDRESS` for agent-wallet setups
//!
//! Required order environment variables:
//! - `HYPERLIQUID_OUTCOME_INSTRUMENT_ID`, using an active
//!   `{outcome_index}-{YES|NO}-OUTCOME.HYPERLIQUID` instrument
//! - `HYPERLIQUID_OUTCOME_ORDER_QTY`, sized to clear the venue minimum notional without exceeding
//!   the available spot balance

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_hyperliquid::{
    HyperliquidDataClientConfig, HyperliquidDataClientFactory, HyperliquidExecutionClientConfig,
    HyperliquidExecutionClientFactory,
    common::{consts::HYPERLIQUID_CLIENT_ID, enums::HyperliquidEnvironment},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

// WARNING: With `DRY_RUN = false`, this tester submits orders to the configured
// environment and may use real funds. Set `DRY_RUN = true` to connect without
// submitting orders or sending shutdown cancel/close commands.
const DRY_RUN: bool = false;
const HYPERLIQUID_ENVIRONMENT: HyperliquidEnvironment = HyperliquidEnvironment::Mainnet;
const TRADER_ID: &str = "TESTER-001";
const ACCOUNT_ID: &str = "HYPERLIQUID-001";
const NODE_NAME: &str = "HYPERLIQUID-OUTCOME-EXEC-TESTER-001";
const STRATEGY_ID: &str = "OUTCOME_EXEC_TESTER-001";

// Pick the index and side from the current `outcomeMeta` snapshot. The venue wire form is
// `#<encoding>` where `encoding = 10 * outcome_index + side` (0 = Yes, 1 = No). Inspect the live
// universe with:
//   curl -s -X POST https://api.hyperliquid.xyz/info \
//     -d '{"type":"outcomeMeta"}'

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let nt_environment = Environment::Live;
    let hl_environment = HYPERLIQUID_ENVIRONMENT;
    let trader_id = TraderId::from(TRADER_ID);
    let account_id = AccountId::from(ACCOUNT_ID);
    let node_name = NODE_NAME.to_string();
    let client_id = *HYPERLIQUID_CLIENT_ID;

    let instrument_id = std::env::var("HYPERLIQUID_OUTCOME_INSTRUMENT_ID")
        .map(InstrumentId::from)
        .map_err(|_| {
            anyhow::anyhow!(
                "HYPERLIQUID_OUTCOME_INSTRUMENT_ID must be set to an active HIP-4 side token"
            )
        })?;
    let order_qty_raw = std::env::var("HYPERLIQUID_OUTCOME_ORDER_QTY")
        .map_err(|_| anyhow::anyhow!("HYPERLIQUID_OUTCOME_ORDER_QTY must be set"))?;
    let order_qty = order_qty_raw.parse::<Quantity>().map_err(|e| {
        anyhow::anyhow!("Invalid HYPERLIQUID_OUTCOME_ORDER_QTY '{order_qty_raw}': {e}")
    })?;

    let data_config = HyperliquidDataClientConfig {
        environment: hl_environment,
        ..Default::default()
    };

    let exec_config = HyperliquidExecutionClientConfig {
        account_id,
        environment: hl_environment,
        ..Default::default()
    };

    let data_factory = HyperliquidDataClientFactory::new();
    let exec_factory = HyperliquidExecutionClientFactory::new();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, nt_environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(10)
        .build()?;

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from(STRATEGY_ID)),
            external_order_claims: Some(vec![instrument_id]),
            use_hyphens_in_client_order_ids: true,
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .dry_run(DRY_RUN)
        .tob_offset_ticks(5)
        .enable_limit_buys(true)
        .enable_limit_sells(false)
        .enable_stop_buys(false)
        .enable_stop_sells(false)
        .enable_brackets(false)
        .use_post_only(true)
        .reduce_only_on_stop(false)
        .cancel_orders_on_stop(true)
        .close_positions_on_stop(false)
        .clamp_to_instrument_price_range(true)
        .log_data(false)
        .build()?;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
