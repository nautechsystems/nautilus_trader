// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_model::{
    data::{quote::QuoteTick, trade::TradeTick},
    enums::{AggressorSide, PriceType},
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    types::{price::Price, quantity::Quantity},
};
use rstest::fixture;

use crate::{
    average::{ema::ExponentialMovingAverage, sma::SimpleMovingAverage},
    ratio::efficiency_ratio::EfficiencyRatio,
};
use crate::average::ama::AdaptiveMovingAverage;

////////////////////////////////////////////////////////////////////////////////
// Common
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn quote_tick() -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        bid_price: Price::from("1500.0000"),
        ask_price: Price::from("1502.0000"),
        bid_size: Quantity::from("1.00000000"),
        ask_size: Quantity::from("1.00000000"),
        ts_event: 1,
        ts_init: 0,
    }
}

#[fixture]
pub fn trade_tick() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        price: Price::from("1500.0000"),
        size: Quantity::from("1.00000000"),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::from("123456789"),
        ts_event: 1,
        ts_init: 0,
    }
}

////////////////////////////////////////////////////////////////////////////////
// Average
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn indicator_ama_10() -> AdaptiveMovingAverage {
    AdaptiveMovingAverage::new(10, 2, 30, Some(PriceType::Mid)).unwrap()
}

#[fixture]
pub fn indicator_sma_10() -> SimpleMovingAverage {
    SimpleMovingAverage::new(10, Some(PriceType::Mid)).unwrap()
}

#[fixture]
pub fn indicator_ema_10() -> ExponentialMovingAverage {
    ExponentialMovingAverage::new(10, Some(PriceType::Mid)).unwrap()
}

#[fixture]
pub fn efficiency_ratio_10() -> EfficiencyRatio {
    EfficiencyRatio::new(10, Some(PriceType::Mid)).unwrap()
}
