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

//! Numerical building blocks shared by indicators: overflow-safe sums and moments,
//! seeded moving-average kernels, and input validation.

mod gain_loss;
mod moments;
mod regression;
mod sum;

pub(crate) use self::{
    gain_loss::percentage_gain,
    moments::ShiftedMoments,
    regression::RollingOls,
    sum::{MAX_PERIOD, SMA_RESEED_WINDOWS, ScaledSum},
};

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
    let sum: ScaledSum = [high, low, close].into_iter().sum();
    sum.mean(3)
}

pub(crate) fn blend(previous: f64, input: f64, alpha: f64) -> f64 {
    let scale = previous.abs().max(input.abs());

    if scale == 0.0 {
        return 0.0;
    }

    (alpha.mul_add(input / scale, (1.0 - alpha) * (previous / scale))).clamp(-1.0, 1.0) * scale
}

pub(crate) fn mean_weighted<I: Iterator<Item = (f64, f64)> + Clone>(values: I) -> f64 {
    let products = values.clone().map(|(value, weight)| value * weight);

    if products.clone().all(f64::is_finite) {
        let numerator: ScaledSum = products.sum();
        let denominator: ScaledSum = values.clone().map(|(_, weight)| weight).sum();
        let mean = numerator.ratio(&denominator);

        if mean.is_finite() {
            return mean;
        }
    }

    let scale = values
        .clone()
        .map(|(value, _)| value.abs())
        .fold(0.0, f64::max);

    if scale == 0.0 {
        return 0.0;
    }

    let numerator: ScaledSum = values
        .clone()
        .map(|(value, weight)| (value / scale) * weight)
        .sum();
    let denominator: ScaledSum = values.clone().map(|(_, weight)| weight).sum();
    let value = numerator.ratio(&denominator);
    let value = if values.map(|(_, weight)| weight).all(|weight| weight >= 0.0) {
        value.clamp(-1.0, 1.0)
    } else {
        value
    };
    value * scale
}
