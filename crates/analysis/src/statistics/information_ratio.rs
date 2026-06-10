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

//! Information ratio statistic (benchmark-relative).

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{
    Returns, statistic::PortfolioStatistic, statistics::tracking_error::active_return_stats,
};

/// Calculates the information ratio of portfolio returns relative to a benchmark.
///
/// The information ratio measures active return per unit of active risk (tracking error):
///
/// `IR = mean(active) / std(active) * sqrt(period)`
///
/// where `active_i = strategy_i - benchmark_i`, `std` uses Bessel's correction
/// (`ddof = 1`), and the ratio is annualized by the square root of the specified period
/// (default: 252 trading days).
///
/// # References
///
/// - Goodwin, T. H. (1998). "The Information Ratio". *Financial Analysts Journal*, 54(4), 34-43.
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
pub struct InformationRatio {
    /// The annualization period (default: 252 for daily data).
    period: usize,
}

impl InformationRatio {
    /// Creates a new [`InformationRatio`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl Display for InformationRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Information Ratio ({} days)", self.period)
    }
}

impl PortfolioStatistic for InformationRatio {
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

        let (mean_active, std_active) = active_return_stats(&r, &b);
        if std_active < f64::EPSILON {
            return Some(f64::NAN);
        }

        let ir_period = mean_active / std_active;
        Some(ir_period * (self.period as f64).sqrt())
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
        let stat = InformationRatio::new(None);
        assert_eq!(stat.name(), "Information Ratio (252 days)");
    }

    #[rstest]
    fn test_name_non_default_period() {
        let stat = InformationRatio::new(Some(63));
        assert_eq!(stat.name(), "Information Ratio (63 days)");
    }

    #[rstest]
    fn test_known_value_nonzero_benchmark() {
        // Both strategy and benchmark non-zero so the (r - b) subtraction is exercised.
        //   r       = [0.03, -0.01, 0.02, 0.04]
        //   b       = [0.01, 0.005, 0.005, 0.01]
        //   active  = [0.02, -0.015, 0.015, 0.03]
        //   mean(active)       = 0.0125
        //   std(active, ddof=1)= 0.019364916731037084
        //   IR = 0.0125 / 0.019364916731037084 * sqrt(252) = 10.246950765959598
        // A formula dropping the benchmark (active = r) would yield 14.69693845669907,
        // so this value discriminates against that bug.
        let returns = create_returns(&[0.03, -0.01, 0.02, 0.04]);
        let benchmark = create_returns(&[0.01, 0.005, 0.005, 0.01]);
        let stat = InformationRatio::new(Some(252));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 10.246950765959598, epsilon = 1e-9));
    }

    #[rstest]
    fn test_partial_overlap_inner_join() {
        // Strategy on days 0..5, benchmark on days 2..7 -> overlap on days 2,3,4 only.
        let one_day = 86_400_000_000_000_u64;
        let start = 1_600_000_000_000_000_000_u64;

        let mut returns = BTreeMap::new();
        for (i, v) in [0.05, 0.06, 0.030, -0.010, 0.020].iter().enumerate() {
            returns.insert(UnixNanos::from(start + i as u64 * one_day), *v);
        }
        let mut benchmark = BTreeMap::new();
        for (i, v) in [0.005, 0.010, 0.015, 0.040, 0.050].iter().enumerate() {
            benchmark.insert(UnixNanos::from(start + (i as u64 + 2) * one_day), *v);
        }

        // Overlap days 2,3,4: r = [0.030, -0.010, 0.020], b = [0.005, 0.010, 0.015].
        //   active = [0.025, -0.020, 0.005]
        //   IR = mean(active) / std(active, ddof=1) * sqrt(252) = 2.3469547761538725
        let stat = InformationRatio::new(Some(252));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 2.3469547761538725, epsilon = 1e-9));
    }

    #[rstest]
    fn test_intraday_compounds_before_join() {
        // Day 0 carries two intraday strategy returns that must compound into one daily
        // bin BEFORE the inner-join: (1.02)(1.03) - 1 = 0.0506. Day 1 carries 0.01.
        // Benchmark is one daily return per day: [0.015, 0.004].
        //   active = [0.0506 - 0.015, 0.01 - 0.004] = [0.0356, 0.006]
        //   IR = mean(active) / std(active, ddof=1) * sqrt(252) = 15.775636549641483
        // Arithmetic-summing the intraday day-0 returns (0.05) would change the result,
        // so this confirms geometric compounding happens before the join.
        let one_day = 86_400_000_000_000_u64;
        let one_hour = 3_600_000_000_000_u64;
        let start = 1_600_000_000_000_000_000_u64;

        let mut returns = BTreeMap::new();
        returns.insert(UnixNanos::from(start), 0.02);
        returns.insert(UnixNanos::from(start + one_hour), 0.03);
        returns.insert(UnixNanos::from(start + one_day), 0.01);

        let mut benchmark = BTreeMap::new();
        benchmark.insert(UnixNanos::from(start), 0.015);
        benchmark.insert(UnixNanos::from(start + one_day), 0.004);

        let active = [(1.02_f64 * 1.03 - 1.0) - 0.015, 0.01 - 0.004];
        let mean_active = active.iter().sum::<f64>() / active.len() as f64;
        let var = active
            .iter()
            .map(|&x| (x - mean_active).powi(2))
            .sum::<f64>()
            / (active.len() as f64 - 1.0);
        let expected = mean_active / var.sqrt() * 252.0_f64.sqrt();

        let stat = InformationRatio::new(Some(252));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, expected, epsilon = 1e-9));
        assert!(approx_eq!(f64, result, 15.775636549641483, epsilon = 1e-9));
    }

    #[rstest]
    fn test_known_value() {
        // active = strategy - benchmark = [0.01, 0.02, 0.03] (a clean arithmetic case).
        // mean = 0.02, sample std (ddof=1) = 0.01, ir_period = 2.0,
        // annualized = 2.0 * sqrt(4) = 4.0.
        let benchmark = create_returns(&[0.00, 0.00, 0.00]);
        let returns = create_returns(&[0.01, 0.02, 0.03]);
        let stat = InformationRatio::new(Some(4));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(f64, result, 4.0, epsilon = 1e-12));
    }

    #[rstest]
    fn test_zero_active_std_is_nan() {
        // strategy - benchmark constant -> std(active) = 0 -> NaN.
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005]);
        let returns = create_returns(&[0.02, -0.01, 0.025, 0.005]);
        let stat = InformationRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_empty_returns_is_nan() {
        let stat = InformationRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&create_returns(&[]), &create_returns(&[]))
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_single_overlap_is_nan() {
        let benchmark = create_returns(&[0.01, -0.02, 0.015]);
        let returns = create_returns(&[0.02]);
        let stat = InformationRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }
}
