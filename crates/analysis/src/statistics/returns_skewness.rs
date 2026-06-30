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

//! Returns Skewness statistic.

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the skewness of portfolio returns.
///
/// Skewness measures the asymmetry of the return distribution about its mean. A
/// negative value indicates a longer left tail (downside outliers); a positive
/// value indicates a longer right tail.
///
/// Uses the bias-corrected sample skewness (adjusted Fisher-Pearson), matching
/// `pandas.Series.skew` and Excel `SKEW`:
///
/// `G1 = n / ((n - 1)(n - 2)) * sum(((x - mean) / s)^3)`
///
/// where `s` is the sample standard deviation (Bessel's correction, ddof=1).
/// Returns `NaN` for fewer than three returns or zero dispersion.
///
/// # References
///
/// - Joanes, D. N., & Gill, C. A. (1998). Comparing measures of sample skewness
///   and kurtosis. *Journal of the Royal Statistical Society: Series D*, 47(1), 183-189.
#[repr(C)]
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.analysis")
)]
pub struct ReturnsSkewness {}

impl ReturnsSkewness {
    /// Creates a new [`ReturnsSkewness`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

impl PortfolioStatistic for ReturnsSkewness {
    type Item = f64;

    fn name(&self) -> String {
        "Returns Skewness".to_string()
    }

    fn calculate_from_returns(&self, raw_returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(raw_returns) {
            return Some(f64::NAN);
        }

        let returns = self.downsample_to_daily_bins(raw_returns);
        let n = returns.len();
        if n < 3 {
            return Some(f64::NAN);
        }

        let n_f = n as f64;
        let mean = returns.values().sum::<f64>() / n_f;
        let std = self.calculate_std(&returns);
        if std == 0.0 || !std.is_finite() {
            return Some(f64::NAN);
        }

        let sum_cubed = returns
            .values()
            .map(|x| ((x - mean) / std).powi(3))
            .sum::<f64>();
        let skewness = n_f / ((n_f - 1.0) * (n_f - 2.0)) * sum_cubed;

        Some(skewness)
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
        let skewness = ReturnsSkewness::new();
        assert_eq!(skewness.name(), "Returns Skewness");
    }

    #[rstest]
    fn test_empty_returns() {
        let skewness = ReturnsSkewness::new();
        let returns = create_returns(&[]);
        let result = skewness.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_insufficient_data() {
        let skewness = ReturnsSkewness::new();
        let returns = create_returns(&[0.01, -0.02]);
        let result = skewness.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_zero_dispersion() {
        let skewness = ReturnsSkewness::new();
        let returns = create_returns(&[0.01, 0.01, 0.01, 0.01]);
        let result = skewness.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_skewness_calculation() {
        // Reference value from pandas Series.skew() (adjusted Fisher-Pearson).
        let skewness = ReturnsSkewness::new();
        let returns = create_returns(&[
            0.01, -0.02, 0.03, -0.01, 0.02, 0.04, -0.03, 0.05, -0.04, 0.02,
        ]);
        let result = skewness.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(approx_eq!(
            f64,
            result.unwrap(),
            -0.22872023422596313,
            epsilon = 1e-12
        ));
    }
}
