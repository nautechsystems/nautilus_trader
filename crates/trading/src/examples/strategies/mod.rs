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

//! Example trading strategies for backtesting and demonstration.

use nautilus_model::{data::QuoteTick, types::price::ERROR_PRICE};
use rust_decimal::Decimal;

pub mod composite_market_maker;
pub mod delta_neutral_vol;
pub mod ema_cross;
pub mod grid_mm;
pub mod hurst_vpin_directional;
pub mod orderbook_imbalance;

pub use composite_market_maker::{CompositeMarketMaker, CompositeMarketMakerConfig};
pub use delta_neutral_vol::{DeltaNeutralVol, DeltaNeutralVolConfig};
pub use ema_cross::{EmaCross, EmaCrossConfig};
pub use grid_mm::{GridMarketMaker, GridMarketMakerConfig};
pub use hurst_vpin_directional::{HurstVpinDirectional, HurstVpinDirectionalConfig};
pub use orderbook_imbalance::{OrderBookImbalance, OrderBookImbalanceConfig};

fn quote_midpoint(quote: &QuoteTick) -> anyhow::Result<Decimal> {
    let bid = quote.bid_price;
    let ask = quote.ask_price;
    anyhow::ensure!(
        !bid.is_error()
            && !bid.is_undefined()
            && bid.precision != ERROR_PRICE.precision
            && !ask.is_error()
            && !ask.is_undefined()
            && ask.precision != ERROR_PRICE.precision,
        "Cannot calculate midpoint for {} from an invalid quote",
        quote.instrument_id,
    );
    Ok((bid.as_decimal() + ask.as_decimal()) / Decimal::TWO)
}

fn price_change_reaches_threshold(change: Decimal, reference: Decimal, bps: u32) -> bool {
    if reference.is_sign_negative() {
        return change.is_zero() && bps == 0;
    }

    change >= reference * Decimal::new(i64::from(bps), 4)
}
