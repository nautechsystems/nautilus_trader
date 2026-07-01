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

//! Tail Ratio statistic.

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the tail ratio of portfolio returns.
///
/// The tail ratio compares the magnitude of the right (gain) tail to the left
/// (loss) tail of the return distribution. It is the absolute ratio of the 95th
/// to the 5th percentile of returns:
///
/// `TailRatio = | percentile(r, 95) / percentile(r, 5) |`
///
/// Percentiles use linear interpolation between closest ranks, matching
/// `numpy.percentile` and `pandas.Series.quantile` with the default `linear`
/// method (the convention used by the `quantstats` tail-ratio definition).
///
/// A value greater than `1` indicates a heavier upside tail (gains larger in
/// magnitude than losses); a value below `1` indicates a heavier downside tail.
/// Returns `NaN` for fewer than two returns or when the 5th percentile is zero.
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
pub struct TailRatio {}

impl TailRatio {
    /// Creates a new [`TailRatio`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

/// Returns the `q`-th percentile (`q` in `[0, 100]`) of `sorted_values` using
/// linear interpolation between closest ranks, matching `numpy.percentile` with
/// the default `linear` method.
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
    // lower + (upper - lower) * weight, fused to match numpy's linear interpolation.
    (sorted_values[upper] - sorted_values[lower]).mul_add(weight, sorted_values[lower])
}

impl PortfolioStatistic for TailRatio {
    type Item = f64;

    fn name(&self) -> String {
        "Tail Ratio".to_string()
    }

    fn calculate_from_returns(&self, raw_returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(raw_returns) {
            return Some(f64::NAN);
        }

        let returns = self.downsample_to_daily_bins(raw_returns);
        let n = returns.len();
        if n < 2 {
            return Some(f64::NAN);
        }

        let mut values: Vec<f64> = returns.values().copied().collect();
        values.sort_by(f64::total_cmp);

        let p95 = percentile_linear(&values, 95.0);
        let p5 = percentile_linear(&values, 5.0);
        if p5 == 0.0 || !p5.is_finite() {
            return Some(f64::NAN);
        }

        Some((p95 / p5).abs())
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
        let tail_ratio = TailRatio::new();
        assert_eq!(tail_ratio.name(), "Tail Ratio");
    }

    #[rstest]
    fn test_empty_returns() {
        let tail_ratio = TailRatio::new();
        let returns = create_returns(&[]);
        let result = tail_ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_insufficient_data() {
        let tail_ratio = TailRatio::new();
        let returns = create_returns(&[0.01]);
        let result = tail_ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_tail_ratio_calculation() {
        // Reference value from numpy.percentile (linear) / pandas.Series.quantile:
        //   |percentile(r, 95)| / |percentile(r, 5)| = 0.0455 / 0.0355 = 91 / 71.
        let tail_ratio = TailRatio::new();
        let returns = create_returns(&[
            0.01, -0.02, 0.03, -0.01, 0.02, 0.04, -0.03, 0.05, -0.04, 0.02,
        ]);
        let result = tail_ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(approx_eq!(
            f64,
            result.unwrap(),
            1.2816901408450704,
            epsilon = 1e-12
        ));
    }

    #[rstest]
    fn test_symmetric_returns_ratio_near_one() {
        // A symmetric distribution has matching tails, so the ratio is ~1.
        let tail_ratio = TailRatio::new();
        let returns = create_returns(&[-0.03, -0.02, -0.01, 0.0, 0.01, 0.02, 0.03]);
        let result = tail_ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 1.0, epsilon = 1e-12));
    }
}
