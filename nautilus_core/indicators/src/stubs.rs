// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
        ema::ExponentialMovingAverage, hma::HullMovingAverage, rma::WilderMovingAverage,
        sma::SimpleMovingAverage, vidya::VariableIndexDynamicAverage, wma::WeightedMovingAverage,
        MovingAverageType,
    },
    momentum::{cmo::ChandeMomentumOscillator, rsi::RelativeStrengthIndex},
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
        bar_type,
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
pub fn indicator_hma_10() -> HullMovingAverage {
    HullMovingAverage::new(10, Some(PriceType::Mid)).unwrap()
}

#[fixture]
pub fn indicator_rma_10() -> WilderMovingAverage {
    WilderMovingAverage::new(10, Some(PriceType::Mid)).unwrap()
}

#[fixture]
pub fn indicator_dema_10() -> DoubleExponentialMovingAverage {
    DoubleExponentialMovingAverage::new(10, Some(PriceType::Mid)).unwrap()
}

#[fixture]
pub fn indicator_vidya_10() -> VariableIndexDynamicAverage {
    VariableIndexDynamicAverage::new(10, Some(PriceType::Mid), Some(MovingAverageType::Wilder))
        .unwrap()
}

#[fixture]
pub fn indicator_wma_10() -> WeightedMovingAverage {
    let weights = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
    WeightedMovingAverage::new(10, weights, Some(PriceType::Mid)).unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// Ratios
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn efficiency_ratio_10() -> EfficiencyRatio {
    EfficiencyRatio::new(10, Some(PriceType::Mid)).unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// Momentum
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn rsi_10() -> RelativeStrengthIndex {
    RelativeStrengthIndex::new(10, Some(MovingAverageType::Exponential)).unwrap()
}

#[fixture]
pub fn cmo_10() -> ChandeMomentumOscillator {
    ChandeMomentumOscillator::new(10, Some(MovingAverageType::Wilder)).unwrap()
}
