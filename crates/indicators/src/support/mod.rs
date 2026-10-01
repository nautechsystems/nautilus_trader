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

//! Numerical building blocks shared by indicators: shifted rolling moments and
//! regression sums, and input validation.

mod moments;
mod regression;

pub(crate) use self::{moments::ShiftedMoments, regression::RollingOls};

pub(crate) const SMA_RESEED_WINDOWS: usize = 16;
/// The maximum period accepted by windowed indicators.
pub const MAX_PERIOD: usize = 1 << 24;

pub(crate) fn log_ratio(numerator: f64, denominator: f64) -> f64 {
    let ratio = numerator / denominator;
    if ratio.is_normal() {
        ratio.ln()
    } else {
        numerator.ln() - denominator.ln()
    }
}

pub(crate) fn is_valid_hlc(high: f64, low: f64, close: f64) -> bool {
    is_valid_high_low(high, low) && close.is_finite() && close >= low && close <= high
}

pub(crate) fn is_valid_high_low(high: f64, low: f64) -> bool {
    high.is_finite() && low.is_finite() && high >= low
}

pub(crate) fn typical_price(high: f64, low: f64, close: f64) -> f64 {
    (high + low + close) / 3.0
}
