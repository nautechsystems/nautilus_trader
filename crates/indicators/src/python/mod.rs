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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod average;
pub mod book;
pub mod momentum;
pub mod ratio;
pub mod volatility;

use pyo3::{prelude::*, pymodule};

#[pymodule]
pub fn indicators(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Average
    m.add_class::<crate::average::MovingAverageType>()?;
    m.add_class::<crate::average::ema::ExponentialMovingAverage>()?;
    m.add_class::<crate::average::sma::SimpleMovingAverage>()?;
    m.add_class::<crate::average::ama::AdaptiveMovingAverage>()?;
    m.add_class::<crate::average::dema::DoubleExponentialMovingAverage>()?;
    m.add_class::<crate::average::hma::HullMovingAverage>()?;
    m.add_class::<crate::average::rma::WilderMovingAverage>()?;
    m.add_class::<crate::average::vidya::VariableIndexDynamicAverage>()?;
    m.add_class::<crate::average::vwap::VolumeWeightedAveragePrice>()?;
    m.add_class::<crate::average::lr::LinearRegression>()?;
    m.add_class::<crate::average::wma::WeightedMovingAverage>()?;

    // Book
    m.add_class::<crate::book::imbalance::BookImbalanceRatio>()?;

    // Ratio
    m.add_class::<crate::ratio::efficiency_ratio::EfficiencyRatio>()?;
    m.add_class::<crate::ratio::spread_analyzer::SpreadAnalyzer>()?;

    // Momentum
    m.add_class::<crate::momentum::rsi::RelativeStrengthIndex>()?;
    m.add_class::<crate::momentum::aroon::AroonOscillator>()?;
    m.add_class::<crate::momentum::bias::Bias>()?;
    m.add_class::<crate::momentum::cmo::ChandeMomentumOscillator>()?;
    m.add_class::<crate::momentum::vhf::VerticalHorizontalFilter>()?;
    m.add_class::<crate::momentum::kvo::KlingerVolumeOscillator>()?;
    m.add_class::<crate::momentum::dm::DirectionalMovement>()?;
    m.add_class::<crate::momentum::amat::ArcherMovingAveragesTrends>()?;
    m.add_class::<crate::momentum::swings::Swings>()?;
    m.add_class::<crate::momentum::bb::BollingerBands>()?;
    m.add_class::<crate::momentum::stochastics::Stochastics>()?;
    m.add_class::<crate::momentum::psl::PsychologicalLine>()?;
    m.add_class::<crate::momentum::pressure::Pressure>()?;
    m.add_class::<crate::momentum::cci::CommodityChannelIndex>()?;
    m.add_class::<crate::momentum::roc::RateOfChange>()?;
    m.add_class::<crate::momentum::macd::MovingAverageConvergenceDivergence>()?;
    m.add_class::<crate::momentum::obv::OnBalanceVolume>()?;

    // Volatility
    m.add_class::<crate::volatility::atr::AverageTrueRange>()?;
    m.add_class::<crate::volatility::vr::VolatilityRatio>()?;
    m.add_class::<crate::volatility::dc::DonchianChannel>()?;
    m.add_class::<crate::volatility::rvi::RelativeVolatilityIndex>()?;
    m.add_class::<crate::volatility::kc::KeltnerChannel>()?;
    m.add_class::<crate::volatility::fuzzy::FuzzyCandlesticks>()?;
    m.add_class::<crate::volatility::kp::KeltnerPosition>()?;
    Ok(())
}
