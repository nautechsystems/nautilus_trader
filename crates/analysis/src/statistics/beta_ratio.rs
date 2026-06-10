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

//! Beta statistic (benchmark-relative).

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the beta of portfolio returns relative to a benchmark.
///
/// Beta measures the systematic risk (market sensitivity) of a portfolio and is
/// calculated as the covariance of the portfolio and benchmark returns divided by
/// the variance of the benchmark returns:
///
/// `Beta = Cov(portfolio, benchmark) / Var(benchmark)`
///
/// Sample (Bessel-corrected, `ddof = 1`) covariance and variance are used to match
/// the standard deviation convention elsewhere in this crate. Beta is not annualized.
///
/// # References
///
/// - Sharpe, W. F. (1964). "Capital Asset Prices: A Theory of Market Equilibrium under
///   Conditions of Risk". *Journal of Finance*, 19(3), 425-442.
/// - CFA Institute Investment Foundations, 3rd Edition
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
pub struct BetaRatio {}

impl BetaRatio {
    /// Creates a new [`BetaRatio`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

impl Display for BetaRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Beta")
    }
}

impl PortfolioStatistic for BetaRatio {
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
        let n = r.len();
        if n < 2 {
            return Some(f64::NAN);
        }

        Some(beta(&r, &b))
    }
}

/// Computes sample (`ddof = 1`) beta of `r` against `b`.
///
/// Returns `f64::NAN` when the benchmark variance is below `f64::EPSILON` (e.g. a flat
/// benchmark), which would otherwise produce a division by zero. Callers must ensure
/// `r.len() == b.len() >= 2`.
pub(crate) fn beta(r: &[f64], b: &[f64]) -> f64 {
    let n = r.len() as f64;
    let mean_r = r.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;

    let covariance = r
        .iter()
        .zip(b.iter())
        .map(|(&ri, &bi)| (ri - mean_r) * (bi - mean_b))
        .sum::<f64>()
        / (n - 1.0);
    let variance_b = b.iter().map(|&bi| (bi - mean_b).powi(2)).sum::<f64>() / (n - 1.0);

    if variance_b < f64::EPSILON {
        return f64::NAN;
    }

    covariance / variance_b
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
        let stat = BetaRatio::new();
        assert_eq!(stat.name(), "Beta");
    }

    #[rstest]
    fn test_known_value() {
        // strategy = 2 * benchmark exactly, so beta = 2.0.
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005, 0.025]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010, 0.050]);
        let stat = BetaRatio::new();
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 2.0, epsilon = 1e-12));
    }

    #[rstest]
    fn test_unit_beta() {
        // Identical series -> beta of 1.0.
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005, 0.025]);
        let returns = create_returns(&[0.01, -0.02, 0.015, -0.005, 0.025]);
        let stat = BetaRatio::new();
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 1.0, epsilon = 1e-12));
    }

    #[rstest]
    fn test_flat_benchmark_is_nan() {
        let benchmark = create_returns(&[0.01, 0.01, 0.01, 0.01, 0.01]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010, 0.050]);
        let stat = BetaRatio::new();
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_empty_returns_is_nan() {
        let stat = BetaRatio::new();
        let result = stat
            .calculate_from_returns_with_benchmark(&create_returns(&[]), &create_returns(&[]))
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_single_overlap_is_nan() {
        // Only one shared timestamp after inner join -> n < 2 -> NaN.
        let benchmark = create_returns(&[0.01, -0.02, 0.015]);
        let returns = create_returns(&[0.02]);
        let stat = BetaRatio::new();
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_partial_overlap_inner_join() {
        // Strategy spans days 0..5, benchmark days 2..7 -> overlap on days 2,3,4 only.
        let one_day = 86_400_000_000_000_u64;
        let start = 1_600_000_000_000_000_000_u64;

        let mut returns = BTreeMap::new();
        for (i, v) in [0.02, -0.04, 0.030, -0.010, 0.050].iter().enumerate() {
            returns.insert(UnixNanos::from(start + i as u64 * one_day), *v);
        }
        let mut benchmark = BTreeMap::new();
        for (i, v) in [0.015, -0.005, 0.025, 0.01, -0.02].iter().enumerate() {
            benchmark.insert(UnixNanos::from(start + (i as u64 + 2) * one_day), *v);
        }

        // Overlap: strategy[2,3,4] = [0.030, -0.010, 0.050];
        //          benchmark[2,3,4] = [0.015, -0.005, 0.025] (here strategy == 2*benchmark).
        let stat = BetaRatio::new();
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 2.0, epsilon = 1e-12));
    }
}
