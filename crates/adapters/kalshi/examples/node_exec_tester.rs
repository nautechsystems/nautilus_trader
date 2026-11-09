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

//! Live Kalshi demo execution testing with the built-in `ExecTester` strategy.
//!
//! WARNING: Risk engine bypass disables ALL shared pre-trade risk checks. Ordinary risk-enabled
//! `LiveNode` submission, order-list, and amendment commands are unsupported with Kalshi's funding
//! model.
//!
//! WARNING: With `DRY_RUN = false`, this example submits real demo orders. It places
//! post-only GTC limits 5 ticks from the touch for `0.01` contracts as a BUY/SELL order list, then
//! cancels those orders on stop. It does not close a position: the built-in close submits
//! a market order, which the Kalshi execution client denies. Startup reconciliation uses a
//! 180-minute lookback. It still keeps, on shard 0, resting orders, orders with a fill in
//! that window, and other orders last updated in that window. A kept order with a fill and
//! no cached order context fails startup. The strategy has no alpha advantage whatsoever and is
//! not intended for production trading.
//!
//! Edit the instrument constant to target an open demo market on shard 0 with a
//! single-step price grid. Keep the bid more than 5 ticks above zero.
//!
//! Run with:
//! `cargo run --locked -p nautilus-kalshi --example kalshi-exec-tester`
//!
//! Demo credentials are read from the environment when both config fields are absent:
//! - `KALSHI_DEMO_API_KEY_ID`
//! - `KALSHI_DEMO_PRIVATE_KEY`

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_kalshi::{
    common::{consts::KALSHI_CLIENT_ID, enums::KalshiEnvironment},
    config::{KalshiDataClientConfig, KalshiExecutionClientConfig},
    factories::{KalshiDataClientFactory, KalshiExecutionClientFactory},
};
use nautilus_live::{
    config::{LiveExecutionEngineConfig, LiveRiskEngineConfig},
    node::LiveNode,
};
use nautilus_model::{
    identifiers::{InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

// WARNING: With `DRY_RUN = false`, this tester submits orders to the demo environment.
// Set `DRY_RUN = true` to connect without submitting orders or sending shutdown
// cancel commands.
const DRY_RUN: bool = false;
const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "KALSHI-EXEC-TESTER-001";
const STRATEGY_ID: &str = "EXEC_TESTER-001";
const INSTRUMENT_ID: &str = "KXNBA1HTOTAL-26NOV01LACGSW-125.KALSHI";
const ORDER_QTY: &str = "0.01";
const TOB_OFFSET_TICKS: u64 = 5;
const EXCHANGE_INDEX: u32 = 0;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let trader_id = TraderId::from(TRADER_ID);
    let client_id = *KALSHI_CLIENT_ID;
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);

    let data_config = KalshiDataClientConfig {
        environment: KalshiEnvironment::Demo,
        instrument_ids: vec![instrument_id],
        ..Default::default()
    };

    let exec_config = KalshiExecutionClientConfig {
        environment: KalshiEnvironment::Demo,
        exchange_index: EXCHANGE_INDEX,
        ..Default::default()
    };

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let exec_engine_config = LiveExecutionEngineConfig {
        reconciliation_lookback_mins: Some(180),
        reconciliation_instrument_ids: Some(vec![instrument_id.to_string()]),
        open_check_interval_secs: Some(10.0),
        position_check_interval_secs: Some(30.0),
        ..Default::default()
    };

    let risk_engine_config = LiveRiskEngineConfig {
        bypass: true,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(NODE_NAME.to_string())
        .with_load_state(false)
        .with_save_state(false)
        .with_logging(log_config)
        .with_exec_engine_config(exec_engine_config)
        .with_risk_engine_config(risk_engine_config)
        .with_reconciliation(true)
        .with_timeout_disconnection_secs(30)
        .with_delay_post_stop_secs(30)
        .add_data_client(
            None,
            Box::new(KalshiDataClientFactory),
            Box::new(data_config),
        )?
        .add_exec_client(
            None,
            Box::new(KalshiExecutionClientFactory),
            Box::new(exec_config),
        )?
        .build()?;

    let order_qty = Quantity::from(ORDER_QTY);

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from(STRATEGY_ID)),
            external_order_instrument_ids: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .enable_limit_buys(true)
        .enable_limit_sells(true)
        .batch_submit_limit_pair(true)
        .enable_stop_buys(false)
        .enable_stop_sells(false)
        .tob_offset_ticks(TOB_OFFSET_TICKS)
        .use_post_only(true)
        .cancel_orders_on_stop(true)
        .close_positions_on_stop(false)
        .dry_run(DRY_RUN)
        .log_data(false)
        .build()?;

    node.add_strategy(ExecTester::new(tester_config))?;
    node.run().await?;
    Ok(())
}
