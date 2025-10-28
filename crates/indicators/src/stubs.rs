// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Type stubs to facilitate testing.

use nautilus_model::{
    data::{Bar, BarSpecification, BarType, QuoteTick, TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::{InstrumentId, Symbol, TradeId, Venue},
    types::{Price, Quantity},
};
use rstest::*;

use crate::{
    average::{
        MovingAverageType, ama::AdaptiveMovingAverage, dema::DoubleExponentialMovingAverage,
        ema::ExponentialMovingAverage, hma::HullMovingAverage, lr::LinearRegression,
        rma::WilderMovingAverage, sma::SimpleMovingAverage, vidya::VariableIndexDynamicAverage,
        vwap::VolumeWeightedAveragePrice, wma::WeightedMovingAverage,
    },
    momentum::{
        amat::ArcherMovingAveragesTrends, bb::BollingerBands, bias::Bias,
        cci::CommodityChannelIndex, cmo::ChandeMomentumOscillator, dm::DirectionalMovement,
        kvo::KlingerVolumeOscillator, macd::MovingAverageConvergenceDivergence,
        obv::OnBalanceVolume, pressure::Pressure, psl::PsychologicalLine, roc::RateOfChange,
        rsi::RelativeStrengthIndex, stochastics::Stochastics, swings::Swings,
        vhf::VerticalHorizontalFilter,
    },
    ratio::{efficiency_ratio::EfficiencyRatio, spread_analyzer::SpreadAnalyzer},
    volatility::{
        dc::DonchianChannel, fuzzy::FuzzyCandlesticks, kc::KeltnerChannel, kp::KeltnerPosition,
        rvi::RelativeVolatilityIndex, vr::VolatilityRatio,
    },
};

////////////////////////////////////////////////////////////////////////////////
// Common
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn stub_quote(
    #[default("1500")] bid_price: &str,
    #[default("1502")] ask_price: &str,
) -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        bid_price: Price::from(bid_price),
        ask_price: Price::from(ask_price),
        bid_size: Quantity::from("1.00000000"),
        ask_size: Quantity::from("1.00000000"),
        ts_event: 1.into(),
        ts_init: 0.into(),
    }
}

#[fixture]
pub fn stub_trade() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        price: Price::from("1500.0000"),
        size: Quantity::from("1.00000000"),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::from("123456789"),
        ts_event: 1.into(),
        ts_init: 0.into(),
    }
}

#[fixture]
pub fn bar_ethusdt_binance_minute_bid(#[default("1522")] close_price: &str) -> Bar {
    let instrument_id = InstrumentId {
        symbol: Symbol::new("ETHUSDT-PERP.BINANCE"),
        venue: Venue::new("BINANCE"),
    };
    let bar_spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Bid);
    let bar_type = BarType::Standard {
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
        ts_event: 0.into(),
        ts_init: 1.into(),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Average
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn indicator_ama_10() -> AdaptiveMovingAverage {
    AdaptiveMovingAverage::new(10, 2, 30, Some(PriceType::Mid))
}

#[fixture]
pub fn indicator_sma_10() -> SimpleMovingAverage {
    SimpleMovingAverage::new(10, Some(PriceType::Mid))
}

#[fixture]
pub fn indicator_ema_10() -> ExponentialMovingAverage {
    ExponentialMovingAverage::new(10, Some(PriceType::Mid))
}

#[fixture]
pub fn indicator_hma_10() -> HullMovingAverage {
    HullMovingAverage::new(10, Some(PriceType::Mid))
}

#[fixture]
pub fn indicator_rma_10() -> WilderMovingAverage {
    WilderMovingAverage::new(10, Some(PriceType::Mid))
}

#[fixture]
pub fn indicator_dema_10() -> DoubleExponentialMovingAverage {
    DoubleExponentialMovingAverage::new(10, Some(PriceType::Mid))
}

#[fixture]
pub fn indicator_vidya_10() -> VariableIndexDynamicAverage {
    VariableIndexDynamicAverage::new(10, Some(PriceType::Mid), Some(MovingAverageType::Wilder))
}

#[fixture]
pub fn indicator_vwap() -> VolumeWeightedAveragePrice {
    VolumeWeightedAveragePrice::new()
}

#[fixture]
pub fn indicator_wma_10() -> WeightedMovingAverage {
    let weights = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
    WeightedMovingAverage::new(10, weights, Some(PriceType::Mid))
}

#[fixture]
pub fn indicator_lr_10() -> LinearRegression {
    LinearRegression::new(10)
}

////////////////////////////////////////////////////////////////////////////////
// Ratios
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn efficiency_ratio_10() -> EfficiencyRatio {
    EfficiencyRatio::new(10, Some(PriceType::Mid))
}

#[fixture]
pub fn spread_analyzer_10() -> SpreadAnalyzer {
    SpreadAnalyzer::new(10, InstrumentId::from("ETHUSDT-PERP.BINANCE"))
}

////////////////////////////////////////////////////////////////////////////////
// Momentum
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn rsi_10() -> RelativeStrengthIndex {
    RelativeStrengthIndex::new(10, Some(MovingAverageType::Exponential))
}

#[fixture]
pub fn cmo_10() -> ChandeMomentumOscillator {
    ChandeMomentumOscillator::new(10, Some(MovingAverageType::Wilder))
}

#[fixture]
pub fn bias_10() -> Bias {
    Bias::new(10, Some(MovingAverageType::Wilder))
}

#[fixture]
pub fn vhf_10() -> VerticalHorizontalFilter {
    VerticalHorizontalFilter::new(10, Some(MovingAverageType::Simple))
}

#[fixture]
pub fn kvo_345() -> KlingerVolumeOscillator {
    KlingerVolumeOscillator::new(3, 4, 5, Some(MovingAverageType::Simple))
}

#[fixture]
pub fn dm_10() -> DirectionalMovement {
    DirectionalMovement::new(10, Some(MovingAverageType::Simple))
}

#[fixture]
pub fn amat_345() -> ArcherMovingAveragesTrends {
    ArcherMovingAveragesTrends::new(3, 4, 5, Some(MovingAverageType::Simple))
}

#[fixture]
pub fn swings_10() -> Swings {
    Swings::new(10)
}

#[fixture]
pub fn bb_10() -> BollingerBands {
    BollingerBands::new(10, 0.1, Some(MovingAverageType::Simple))
}

#[fixture]
pub fn stochastics_10() -> Stochastics {
    Stochastics::new(10, 10)
}

#[fixture]
pub fn psl_10() -> PsychologicalLine {
    PsychologicalLine::new(10, Some(MovingAverageType::Simple))
}

#[fixture]
pub fn pressure_10() -> Pressure {
    Pressure::new(10, Some(MovingAverageType::Simple), Some(1.0))
}

#[fixture]
pub fn cci_10() -> CommodityChannelIndex {
    CommodityChannelIndex::new(10, 2.0, Some(MovingAverageType::Simple))
}

#[fixture]
pub fn macd_10() -> MovingAverageConvergenceDivergence {
    MovingAverageConvergenceDivergence::new(
        10,
        8,
        Some(MovingAverageType::Simple),
        Some(PriceType::Bid),
    )
}

#[fixture]
pub fn obv_10() -> OnBalanceVolume {
    OnBalanceVolume::new(10)
}

////////////////////////////////////////////////////////////////////////////////
// Volatility
////////////////////////////////////////////////////////////////////////////////
#[fixture]
pub fn vr_10() -> VolatilityRatio {
    VolatilityRatio::new(
        10,
        10,
        Some(MovingAverageType::Simple),
        Some(false),
        Some(10.0),
    )
}

#[fixture]
pub fn dc_10() -> DonchianChannel {
    DonchianChannel::new(10)
}

#[fixture]
pub fn rvi_10() -> RelativeVolatilityIndex {
    RelativeVolatilityIndex::new(10, Some(10.0), Some(MovingAverageType::Simple))
}

#[fixture]
pub fn kc_10() -> KeltnerChannel {
    KeltnerChannel::new(
        10,
        2.0,
        Some(MovingAverageType::Simple),
        Some(MovingAverageType::Simple),
        Some(true),
        Some(0.0),
    )
}

#[fixture]
pub fn kp_10() -> KeltnerPosition {
    KeltnerPosition::new(
        10,
        2.0,
        Some(MovingAverageType::Simple),
        Some(MovingAverageType::Simple),
        Some(true),
        Some(0.0),
    )
}

#[fixture]
pub fn roc_10() -> RateOfChange {
    RateOfChange::new(10, Some(true))
}

#[fixture]
pub fn fuzzy_candlesticks_10() -> FuzzyCandlesticks {
    FuzzyCandlesticks::new(10, 0.1, 0.15, 0.2, 0.3)
}

#[fixture]
pub fn fuzzy_candlesticks_1() -> FuzzyCandlesticks {
    FuzzyCandlesticks::new(1, 0.1, 0.15, 0.2, 0.3)
}

#[fixture]
pub fn fuzzy_candlesticks_3() -> FuzzyCandlesticks {
    FuzzyCandlesticks::new(3, 0.1, 0.15, 0.2, 0.3)
}
