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

//! Tracking error statistic (benchmark-relative).

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the tracking error of portfolio returns relative to a benchmark.
///
/// Tracking error is the volatility of the active return (portfolio minus benchmark):
///
/// `TE = std(active) * sqrt(period)`
///
/// where `active_i = portfolio_i - benchmark_i`, `std` uses Bessel's correction
/// (`ddof = 1`), and the result is annualized by the square root of the specified period
/// (default: 252 trading days).
///
/// # References
///
/// - Roll, R. (1992). "A Mean/Variance Analysis of Tracking Error".
///   *Journal of Portfolio Management*, 18(4), 13-22.
/// - CFA Institute Investment Foundations, 3rd Edition
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
pub struct TrackingError {
    /// The annualization period (default: 252 for daily data).
    period: usize,
}

impl TrackingError {
    /// Creates a new [`TrackingError`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl Display for TrackingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tracking Error ({} days)", self.period)
    }
}

impl PortfolioStatistic for TrackingError {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_returns(&self, _returns: &Returns) -> Option<Self::Item> {
        None
    }

    fn calculate_from_realized_pnls(&self, _realized_pnls: &[f64]) -> Option<Self::Item> {
        None
    }

    fn calculate_from_positions(&self, _positions: &[Position]) -> Option<Self::Item> {
        None
    }

    fn calculate_from_returns_with_benchmark(
        &self,
        returns: &Returns,
        benchmark: &Returns,
    ) -> Option<Self::Item> {
        let (r, b) = self.align_returns(returns, benchmark);
        if r.len() < 2 {
            return Some(f64::NAN);
        }

        let (_, std_active) = active_return_stats(&r, &b);
        Some(std_active * (self.period as f64).sqrt())
    }
}

/// Computes the mean and sample (`ddof = 1`) standard deviation of the active
/// return series `r - b`.
///
/// Callers must ensure `r.len() == b.len() >= 2`.
pub(crate) fn active_return_stats(r: &[f64], b: &[f64]) -> (f64, f64) {
    let n = r.len() as f64;
    let mean = r
        .iter()
        .zip(b.iter())
        .map(|(&ri, &bi)| ri - bi)
        .sum::<f64>()
        / n;
    let variance = r
        .iter()
        .zip(b.iter())
        .map(|(&ri, &bi)| (ri - bi - mean).powi(2))
        .sum::<f64>()
        / (n - 1.0);
    (mean, variance.sqrt())
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
        let stat = TrackingError::new(None);
        assert_eq!(stat.name(), "Tracking Error (252 days)");
    }

    #[rstest]
    fn test_name_non_default_period() {
        let stat = TrackingError::new(Some(63));
        assert_eq!(stat.name(), "Tracking Error (63 days)");
    }

    #[rstest]
    fn test_known_value_nonzero_benchmark() {
        // Both strategy and benchmark non-zero so the (r - b) subtraction is exercised.
        //   r       = [0.03, -0.01, 0.02, 0.04]
        //   b       = [0.01, 0.005, 0.005, 0.01]
        //   active  = [0.02, -0.015, 0.015, 0.03]
        //   std(active, ddof=1) = 0.019364916731037084
        //   TE = std(active, ddof=1) * sqrt(252) = 0.30740852297878796
        // A formula dropping the benchmark (active = r) would yield 0.34292856398964494,
        // so this value discriminates against that bug.
        let returns = create_returns(&[0.03, -0.01, 0.02, 0.04]);
        let benchmark = create_returns(&[0.01, 0.005, 0.005, 0.01]);
        let stat = TrackingError::new(Some(252));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 0.30740852297878796, epsilon = 1e-9));
    }

    #[rstest]
    fn test_known_value() {
        // active = [0.01, 0.02, 0.03], sample std (ddof=1) = 0.01,
        // annualized = 0.01 * sqrt(4) = 0.02.
        let benchmark = create_returns(&[0.00, 0.00, 0.00]);
        let returns = create_returns(&[0.01, 0.02, 0.03]);
        let stat = TrackingError::new(Some(4));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 0.02, epsilon = 1e-12));
    }

    #[rstest]
    fn test_zero_active_is_zero() {
        // Identical series -> active all zero -> TE = 0 (not NaN).
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005]);
        let returns = create_returns(&[0.01, -0.02, 0.015, -0.005]);
        let stat = TrackingError::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 0.0, epsilon = 1e-12));
    }

    #[rstest]
    fn test_empty_returns_is_nan() {
        let stat = TrackingError::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&create_returns(&[]), &create_returns(&[]))
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_single_overlap_is_nan() {
        let benchmark = create_returns(&[0.01, -0.02, 0.015]);
        let returns = create_returns(&[0.02]);
        let stat = TrackingError::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }
}
