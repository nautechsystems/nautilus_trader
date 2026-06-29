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

//! Value at Risk statistic.

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the historical Value at Risk (`VaR`) of portfolio returns.
///
/// `VaR` is the loss threshold that returns are not expected to exceed at a given
/// confidence level. This is the non-parametric (historical) estimator: the
/// empirical quantile of the return distribution at `1 - confidence`.
///
/// `VaR(c) = quantile(returns, 1 - c)`
///
/// The quantile uses linear interpolation between closest ranks (matching
/// `numpy.percentile`). `confidence` defaults to `0.95`. The result is expressed
/// as a return (e.g. `-0.03` is a 3% loss threshold); more negative means greater
/// risk. Returns `NaN` for an empty series.
#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.analysis")
)]
pub struct ValueAtRisk {
    /// The confidence level `c` in `(0, 1)` (default: 0.95).
    confidence: f64,
}

impl ValueAtRisk {
    /// Creates a new [`ValueAtRisk`] instance.
    #[must_use]
    pub fn new(confidence: Option<f64>) -> Self {
        Self {
            confidence: confidence.unwrap_or(0.95),
        }
    }
}

impl Display for ValueAtRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Value at Risk (confidence {})", self.confidence)
    }
}

/// Returns the `q`-th percentile (`q` in `[0, 100]`) of `sorted_values` using
/// linear interpolation between closest ranks, matching `numpy.percentile`.
///
/// `sorted_values` must be sorted ascending and non-empty.
fn percentile_linear(sorted_values: &[f64], q: f64) -> f64 {
    debug_assert!(
        !sorted_values.is_empty(),
        "percentile requires a non-empty slice"
    );
    let n = sorted_values.len();
    if n == 1 {
        return sorted_values[0];
    }

    let rank = (q / 100.0) * (n - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        return sorted_values[lower];
    }

    let weight = rank - lower as f64;
    (sorted_values[upper] - sorted_values[lower]).mul_add(weight, sorted_values[lower])
}

impl PortfolioStatistic for ValueAtRisk {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_returns(&self, raw_returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(raw_returns) {
            return Some(f64::NAN);
        }

        let returns = self.downsample_to_daily_bins(raw_returns);
        let mut values: Vec<f64> = returns.values().copied().collect();
        values.sort_by(f64::total_cmp);

        let alpha = 1.0 - self.confidence;
        Some(percentile_linear(&values, alpha * 100.0))
    }

    fn calculate_from_realized_pnls(&self, _realized_pnls: &[f64]) -> Option<Self::Item> {
        None
    }

    fn calculate_from_positions(&self, _positions: &[Position]) -> Option<Self::Item> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nautilus_core::{UnixNanos, approx_eq};
    use rstest::rstest;

    use super::*;

    fn create_returns(values: &[f64]) -> BTreeMap<UnixNanos, f64> {
        let mut new_return = BTreeMap::new();
        let one_day_in_nanos = 86_400_000_000_000;
        let start_time = 1_600_000_000_000_000_000;

        for (i, &value) in values.iter().enumerate() {
            let timestamp = start_time + i as u64 * one_day_in_nanos;
            new_return.insert(UnixNanos::from(timestamp), value);
        }

        new_return
    }

    #[rstest]
    fn test_name() {
        let var = ValueAtRisk::new(None);
        assert_eq!(var.name(), "Value at Risk (confidence 0.95)");
    }

    #[rstest]
    fn test_empty_returns() {
        let var = ValueAtRisk::new(None);
        let returns = create_returns(&[]);
        let result = var.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_value_at_risk_calculation() {
        // sorted: [-0.10, -0.08, -0.05, -0.03, -0.02, 0.01, 0.015, 0.02, 0.03, 0.04]
        // alpha = 0.05, rank = 0.05 * 9 = 0.45
        // VaR = -0.10 + (-0.08 - -0.10) * 0.45 = -0.091
        let var = ValueAtRisk::new(Some(0.95));
        let returns = create_returns(&[
            0.02, -0.05, 0.01, -0.08, 0.03, -0.02, 0.04, -0.10, 0.015, -0.03,
        ]);
        let result = var.calculate_from_returns(&returns).unwrap();
        assert!(approx_eq!(f64, result, -0.091, epsilon = 1e-12));
    }
}
