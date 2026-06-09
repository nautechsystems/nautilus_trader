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

//! Treynor ratio statistic (benchmark-relative).

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic, statistics::beta_ratio::beta};

/// Calculates the Treynor ratio of portfolio returns relative to a benchmark.
///
/// The Treynor ratio measures excess return per unit of systematic risk (beta):
///
/// `Treynor = (annualized_return - rf_annual) / beta`
///
/// The strategy's annualized return is computed geometrically (CAGR-style) from the
/// aligned returns: `annualized_return = (prod(1 + r_i))^(period / n) - 1`. The
/// per-period risk-free rate is annualized geometrically as
/// `rf_annual = (1 + rf)^period - 1`. Beta is the sample (`ddof = 1`) beta of the
/// strategy against the benchmark. The period defaults to 252 trading days and `rf`
/// defaults to 0.0.
///
/// # References
///
/// - Treynor, J. L. (1965). "How to Rate Management of Investment Funds".
///   *Harvard Business Review*, 43(1), 63-75.
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
pub struct TreynorRatio {
    /// The annualization period (default: 252 for daily data).
    period: usize,
    /// The per-period risk-free rate (default: 0.0).
    risk_free_rate: f64,
}

impl TreynorRatio {
    /// Creates a new [`TreynorRatio`] instance.
    #[must_use]
    pub fn new(period: Option<usize>, risk_free_rate: Option<f64>) -> Self {
        Self {
            period: period.unwrap_or(252),
            risk_free_rate: risk_free_rate.unwrap_or(0.0),
        }
    }
}

impl Display for TreynorRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Treynor Ratio ({} days)", self.period)
    }
}

impl PortfolioStatistic for TreynorRatio {
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
        if beta.is_nan() || beta.abs() < f64::EPSILON {
            return Some(f64::NAN);
        }

        let period = self.period as f64;
        let growth = r.iter().map(|&ri| 1.0 + ri).product::<f64>();
        let annualized_return = growth.powf(period / n as f64) - 1.0;
        let rf_annual = (1.0 + self.risk_free_rate).powf(period) - 1.0;

        Some((annualized_return - rf_annual) / beta)
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
        let stat = TreynorRatio::new(None, None);
        assert_eq!(stat.name(), "Treynor Ratio (252 days)");
    }

    #[rstest]
    fn test_known_value() {
        // strategy = 2 * benchmark -> beta = 2 (rf = 0).
        // aligned strategy = [0.02, -0.04, 0.030, -0.010, 0.050], n = 5, period = 5.
        // growth = prod(1+r_i); annualized = growth^(5/5) - 1 = growth - 1.
        // treynor = (growth - 1) / 2.
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005, 0.025]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010, 0.050]);
        let stat = TreynorRatio::new(Some(5), Some(0.0));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();

        let growth = 1.02_f64 * 0.96 * 1.03 * 0.99 * 1.05;
        let expected = (growth - 1.0) / 2.0;
        assert!(approx_eq!(f64, result, expected, epsilon = 1e-12));
    }

    #[rstest]
    fn test_name_non_default_period() {
        let stat = TreynorRatio::new(Some(63), None);
        assert_eq!(stat.name(), "Treynor Ratio (63 days)");
    }

    #[rstest]
    fn test_known_value_period_ne_n_and_nonzero_rf() {
        // period != n (so the (period / n) exponent is not 1) and rf != 0 (so rf must be
        // annualized geometrically). n = 4, period = 252, rf = 0.0001 per period.
        //   r = [0.02, -0.01, 0.03, 0.005], b = [0.01, 0.0, 0.015, 0.01]
        //   beta = 2.5789473684210527
        //   growth = prod(1 + r) = 1.04529447
        //   annualized_geom = growth^(252/4) - 1 = 15.294278229155484
        //   rf_annual = (1 + 0.0001)^252 - 1 = 0.025518911987694626
        //   treynor = (annualized_geom - rf_annual) / beta = 5.920539327065061
        // A wrong exponent (^1) gives 0.0076..., and skipping rf-annualization gives
        // 5.9303956..., so this value discriminates against both bugs.
        let returns = create_returns(&[0.02, -0.01, 0.03, 0.005]);
        let benchmark = create_returns(&[0.01, 0.0, 0.015, 0.01]);
        let stat = TreynorRatio::new(Some(252), Some(0.0001));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 5.920539327065061, epsilon = 1e-9));
    }

    #[rstest]
    fn test_zero_beta_is_nan() {
        // Flat benchmark -> beta NaN -> NaN.
        let benchmark = create_returns(&[0.01, 0.01, 0.01, 0.01, 0.01]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010, 0.050]);
        let stat = TreynorRatio::new(None, None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_empty_returns_is_nan() {
        let stat = TreynorRatio::new(None, None);
        let result = stat
            .calculate_from_returns_with_benchmark(&create_returns(&[]), &create_returns(&[]))
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_single_overlap_is_nan() {
        let benchmark = create_returns(&[0.01, -0.02, 0.015]);
        let returns = create_returns(&[0.02]);
        let stat = TreynorRatio::new(None, None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }
}
