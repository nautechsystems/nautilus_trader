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

use pyo3::{prelude::*, pymodule};

pub mod average;
pub mod book;
pub mod momentum;
pub mod ratio;
pub mod volatility;

#[pymodule]
pub fn indicators(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    // average
    m.add_class::<crate::average::ema::ExponentialMovingAverage>()?;
    m.add_class::<crate::average::sma::SimpleMovingAverage>()?;
    m.add_class::<crate::average::ama::AdaptiveMovingAverage>()?;
    m.add_class::<crate::average::dema::DoubleExponentialMovingAverage>()?;
    m.add_class::<crate::average::hma::HullMovingAverage>()?;
    m.add_class::<crate::average::rma::WilderMovingAverage>()?;
    m.add_class::<crate::average::vidya::VariableIndexDynamicAverage>()?;
    // book
    m.add_class::<crate::book::imbalance::BookImbalanceRatio>()?;
    // ratio
    m.add_class::<crate::ratio::efficiency_ratio::EfficiencyRatio>()?;
    // momentum
    m.add_class::<crate::momentum::rsi::RelativeStrengthIndex>()?;
    m.add_class::<crate::momentum::aroon::AroonOscillator>()?;
    m.add_class::<crate::momentum::cmo::ChandeMomentumOscillator>()?;
    // volatility
    m.add_class::<crate::volatility::atr::AverageTrueRange>()?;
    Ok(())
}
