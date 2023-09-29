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
    data::{
        bar::{Bar, BarSpecification, BarType},
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, trade_id::TradeId, venue::Venue},
    types::{price::Price, quantity::Quantity},
};
use rstest::*;

use crate::{
    average::{
        ama::AdaptiveMovingAverage, dema::DoubleExponentialMovingAverage,
        ema::ExponentialMovingAverage, sma::SimpleMovingAverage,
    },
    ratio::efficiency_ratio::EfficiencyRatio,
};

////////////////////////////////////////////////////////////////////////////////
// Common
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn quote_tick(
    #[default("1500")] bid_price: &str,
    #[default("1502")] ask_price: &str,
) -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        bid_price: Price::from(bid_price),
        ask_price: Price::from(ask_price),
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

#[fixture]
pub fn bar_ethusdt_binance_minute_bid(#[default("1522")] close_price: &str) -> Bar {
    let instrument_id = InstrumentId {
        symbol: Symbol::new("ETHUSDT-PERP.BINANCE").unwrap(),
        venue: Venue::new("BINANCE").unwrap(),
    };
    let bar_spec = BarSpecification {
        step: 1,
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Bid,
    };
    let bar_type = BarType {
        instrument_id,
        spec: bar_spec,
        aggregation_source: AggregationSource::External,
    };
    Bar {
        bar_type: bar_type,
        open: Price::from("1500.0"),
        high: Price::from("1550.0"),
        low: Price::from("1495.0"),
        close: Price::from(close_price),
        volume: Quantity::from("100000"),
        ts_event: 0,
        ts_init: 1,
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
pub fn indicator_dema_10() -> DoubleExponentialMovingAverage {
    DoubleExponentialMovingAverage::new(10, Some(PriceType::Mid)).unwrap()
}

#[fixture]
pub fn efficiency_ratio_10() -> EfficiencyRatio {
    EfficiencyRatio::new(10, Some(PriceType::Mid)).unwrap()
}
