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

//! Example demonstrating the GridMarketMaker strategy with exchange selection.
//!
//! Change `EXCHANGE_STR` below to switch between dYdX and Bybit.
//!
//! Run with: `cargo run --package bin`
//!
//! Required credential environment variables vary by exchange:
//! - dYdX: `DYDX_PRIVATE_KEY` (or `DYDX_TESTNET_PRIVATE_KEY` for testnet).
//! - Bybit: `BYBIT_API_KEY`, `BYBIT_API_SECRET`.

mod exchange;
mod strategy;

use exchange::Exchange;
use nautilus_model::{
    identifiers::TraderId,
    types::Quantity,
};
use crate::strategy::grid_mm::{config::GridMarketMakerConfig, strategy::GridMarketMaker};
// use nautilus_trading::examples::strategies::{GridMarketMaker, GridMarketMakerConfig};
// us

// use strategy::gri
const EXCHANGE_STR: &str = "dydx";

const TRADER_ID: &str = "TESTER-001";

const MAX_POSITION: &str = "0.0001";
const TRADE_SIZE: &str = "0.0001";
const NUM_LEVELS: usize = 1;
const GRID_STEP_BPS: u32 = 7;
const SKEW_FACTOR: f64 = 0.75;
const REQUOTE_THRESHOLD_BPS: u32 = 5;
const EXPIRE_TIME_SECS: u64 = 4;
const ON_CANCEL_RESUBMIT: bool = true;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let exchange: Exchange = EXCHANGE_STR.parse()?;
    let trader_id = TraderId::from(TRADER_ID);

    let (mut node, instrument_id) = exchange.build_node(trader_id)?;

    let config = GridMarketMakerConfig::builder()
        .instrument_id(instrument_id)
        .max_position(Quantity::from(MAX_POSITION))
        .trade_size(Quantity::from(TRADE_SIZE))
        .num_levels(NUM_LEVELS)
        .grid_step_bps(GRID_STEP_BPS)
        .skew_factor(SKEW_FACTOR)
        .requote_threshold_bps(REQUOTE_THRESHOLD_BPS)
        .expire_time_secs(EXPIRE_TIME_SECS)
        .on_cancel_resubmit(ON_CANCEL_RESUBMIT)
        .build();
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}