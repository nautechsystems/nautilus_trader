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
//! Mirrors `examples/live/hyperliquid/hyperliquid_outcomes_exec_tester.py` so
//! the same Yes-side BTC daily market is exercised from the Rust live node.
//!
//! Prerequisites:
//! - Set `HYPERLIQUID_PK` (mainnet) or `HYPERLIQUID_TESTNET_PK` (testnet)
//! - Optionally `HYPERLIQUID_ACCOUNT_ADDRESS` for agent-wallet setups
//!
//! Run with:
//! `cargo run --example hyperliquid-outcome-exec-tester --package nautilus-hyperliquid --features examples`

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_hyperliquid::{
    HyperliquidDataClientConfig, HyperliquidDataClientFactory, HyperliquidExecClientConfig,
    HyperliquidExecFactoryConfig, HyperliquidExecutionClientFactory,
    common::{consts::HYPERLIQUID_CLIENT_ID, enums::HyperliquidEnvironment},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let nt_environment = Environment::Live;
    let hl_environment = HyperliquidEnvironment::Mainnet;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let node_name = "HYPERLIQUID-OUTCOME-EXEC-TESTER-001".to_string();
    let client_id = *HYPERLIQUID_CLIENT_ID;

    // Targets a HIP-4 outcome side token by Nautilus instrument id (`+E`).
    // Pick the encoding for the desired outcome / side from the current
    // `outcomeMeta` snapshot: `encoding = 10 * outcome_index + outcome_side`,
    // side 0 = Yes, side 1 = No. The venue's spot-coin form is `#E`.
    // Inspect the live universe with:
    //   curl -s -X POST https://api.hyperliquid.xyz/info \
    //     -d '{"type":"outcomeMeta"}'
    let instrument_id = InstrumentId::from("+250.HYPERLIQUID");

    let data_config = HyperliquidDataClientConfig {
        environment: hl_environment,
        ..Default::default()
    };

    let exec_config = HyperliquidExecFactoryConfig {
        trader_id,
        account_id,
        config: HyperliquidExecClientConfig {
            environment: hl_environment,
            ..Default::default()
        },
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

    // Outcome size precision is 2 (lot 0.01). Pick `order_qty` such that
    // `order_qty * limit_price` stays within the spot USDH balance and clears
    // the 10 USDH venue minimum notional. Sized for a settled-near-final Yes
    // mid (~0.85): 20 contracts at 0.85 = $17 notional, fits a ~$20 USDH
    // balance with headroom. Drop this if the live mid is much lower.
    let order_qty = Quantity::from("20");

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("OUTCOME_EXEC_TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            use_hyphens_in_client_order_ids: true,
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .tob_offset_ticks(5)
        .enable_limit_sells(false)
        .use_post_only(true)
        .reduce_only_on_stop(false)
        .cancel_orders_on_stop(true)
        .log_data(false)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
