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

use pyo3::{prelude::*, types::PyModule, Python};

pub mod average;
pub mod indicator;
pub mod momentum;
pub mod ratio;

#[cfg(test)]
mod stubs;

/// Loaded as nautilus_pyo3.indicators
#[pymodule]
pub fn indicators(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    // average
    m.add_class::<average::ema::ExponentialMovingAverage>()?;
    m.add_class::<average::sma::SimpleMovingAverage>()?;
    m.add_class::<average::ama::AdaptiveMovingAverage>()?;
    m.add_class::<average::dema::DoubleExponentialMovingAverage>()?;
    // ratio
    m.add_class::<ratio::efficiency_ratio::EfficiencyRatio>()?;
    // momentum
    m.add_class::<momentum::rsi::RelativeStrengthIndex>()?;
    Ok(())
}
