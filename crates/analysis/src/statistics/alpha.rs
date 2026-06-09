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

//! Jensen's alpha statistic (benchmark-relative).

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic, statistics::beta_ratio::beta};

/// Calculates Jensen's alpha of portfolio returns relative to a benchmark.
///
/// Alpha measures the excess return of a portfolio over the return predicted by its
/// beta exposure to the benchmark (CAPM). The per-period alpha is:
///
/// `alpha = (mean_strategy - rf) - beta * (mean_benchmark - rf)`
///
/// where `beta` is the sample (`ddof = 1`) beta of the strategy against the benchmark.
/// The per-period alpha is then annualized geometrically over `period` (default 252):
///
/// `alpha_annual = (1 + alpha)^period - 1`
///
/// The risk-free rate `rf` is specified per period (default 0.0).
///
/// # References
///
/// - Jensen, M. C. (1968). "The Performance of Mutual Funds in the Period 1945-1964".
///   *Journal of Finance*, 23(2), 389-416.
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
pub struct Alpha {
    /// The annualization period (default: 252 for daily data).
    period: usize,
    /// The per-period risk-free rate (default: 0.0).
    risk_free_rate: f64,
}

impl Alpha {
    /// Creates a new [`Alpha`] instance.
    #[must_use]
    pub fn new(period: Option<usize>, risk_free_rate: Option<f64>) -> Self {
        Self {
            period: period.unwrap_or(252),
            risk_free_rate: risk_free_rate.unwrap_or(0.0),
        }
    }
}

impl Display for Alpha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Alpha ({} days)", self.period)
    }
}

impl PortfolioStatistic for Alpha {
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

        let beta = beta(&r, &b);
        if beta.is_nan() {
            return Some(f64::NAN);
        }

        let mean_r = r.iter().sum::<f64>() / n as f64;
        let mean_b = b.iter().sum::<f64>() / n as f64;
        let rf = self.risk_free_rate;

        let alpha_period = (mean_r - rf) - beta * (mean_b - rf);
        let alpha_annual = (1.0 + alpha_period).powf(self.period as f64) - 1.0;

        Some(alpha_annual)
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
        let stat = Alpha::new(None, None);
        assert_eq!(stat.name(), "Alpha (252 days)");
    }

    #[rstest]
    fn test_known_value_zero_alpha() {
        // strategy == 2 * benchmark with rf = 0: beta = 2,
        //   alpha_period = mean_r - 2 * mean_b = 0 (since mean_r = 2 * mean_b),
        //   alpha_annual = (1 + 0)^period - 1 = 0.
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005, 0.025]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010, 0.050]);
        let stat = Alpha::new(Some(252), Some(0.0));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 0.0, epsilon = 1e-12));
    }

    #[rstest]
    fn test_known_value_constant_offset() {
        // strategy = benchmark + 0.001 each day -> beta = 1, mean_r - mean_b = 0.001,
        //   alpha_period = 0.001, alpha_annual = 1.001^period - 1 with period = 4.
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005]);
        let returns = create_returns(&[0.011, -0.019, 0.016, -0.004]);
        let stat = Alpha::new(Some(4), Some(0.0));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        let expected = 1.001_f64.powf(4.0) - 1.0;
        assert!(approx_eq!(f64, result, expected, epsilon = 1e-12));
    }

    #[rstest]
    fn test_name_non_default_period() {
        let stat = Alpha::new(Some(4), None);
        assert_eq!(stat.name(), "Alpha (4 days)");
    }

    #[rstest]
    fn test_known_value_nonzero_mean_benchmark() {
        // Non-zero-mean benchmark and a non-trivial beta so the beta * (mean_b - rf)
        // subtraction is exercised; small period = 4 makes the geometric annualization
        // hand-checkable. rf = 0.001 per period.
        //   r = [0.02, -0.01, 0.03, 0.005], b = [0.01, 0.0, 0.015, 0.01]
        //   beta = 2.5789473684210527
        //   mean_r = 0.01125, mean_b = 0.00875
        //   alpha_period = (mean_r - rf) - beta * (mean_b - rf)
        //                = 0.01025 - 2.5789473684210527 * 0.00775 = -0.009736842105263162
        //   alpha_annual = (1 + alpha_period)^4 - 1 = -0.0383822153156389
        // Dropping the beta term gives +0.041634..., and a sqrt-style annualization
        // (alpha_period * sqrt(4)) gives -0.019473..., so this discriminates both.
        let returns = create_returns(&[0.02, -0.01, 0.03, 0.005]);
        let benchmark = create_returns(&[0.01, 0.0, 0.015, 0.01]);
        let stat = Alpha::new(Some(4), Some(0.001));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, -0.0383822153156389, epsilon = 1e-9));
    }

    #[rstest]
    fn test_flat_benchmark_is_nan() {
        let benchmark = create_returns(&[0.01, 0.01, 0.01, 0.01, 0.01]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010, 0.050]);
        let stat = Alpha::new(None, None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_empty_returns_is_nan() {
        let stat = Alpha::new(None, None);
        let result = stat
            .calculate_from_returns_with_benchmark(&create_returns(&[]), &create_returns(&[]))
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_single_overlap_is_nan() {
        let benchmark = create_returns(&[0.01, -0.02, 0.015]);
        let returns = create_returns(&[0.02]);
        let stat = Alpha::new(None, None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }
}
