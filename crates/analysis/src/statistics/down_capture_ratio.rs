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

//! Down capture ratio statistic (benchmark-relative).

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{
    Returns,
    statistic::PortfolioStatistic,
    statistics::up_capture_ratio::{MarketSide, capture_ratio},
};

/// Calculates the down capture ratio of portfolio returns relative to a benchmark.
///
/// The down capture ratio measures how the portfolio performed, on average, during the
/// periods when the benchmark return was negative. It is the ratio of the portfolio's
/// geometric annualized return to the benchmark's geometric annualized return, both
/// computed over the down-market subset only:
///
/// `DownCapture = annualized_return(portfolio | benchmark < 0) / annualized_return(benchmark | benchmark < 0)`
///
/// where each side's annualized return is the geometric (CAGR-style) value
/// `(prod(1 + x_i))^(period / m) - 1` and `m` is the number of down-market periods (the
/// size of the filtered subset, not the full aligned length). The period defaults to
/// 252 trading days. A value below 1.0 means the portfolio lost less than the benchmark
/// in down markets (smaller drawdowns), which is desirable.
///
/// This is the `empyrical.down_capture` convention (geometric annualized-return ratio
/// over the `benchmark < 0` subset). Note that this differs from the Morningstar
/// definition, which uses a ratio of *cumulative* (non-annualized) returns; the two
/// coincide only when both subsets contain the same number of periods.
///
/// # References
///
/// - empyrical `down_capture` / `capture` / `annual_return`
///   (<https://github.com/quantopian/empyrical>).
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
pub struct DownCaptureRatio {
    /// The annualization period (default: 252 for daily data).
    period: usize,
}

impl DownCaptureRatio {
    /// Creates a new [`DownCaptureRatio`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl Display for DownCaptureRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Down Capture Ratio ({} days)", self.period)
    }
}

impl PortfolioStatistic for DownCaptureRatio {
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

        Some(capture_ratio(&r, &b, self.period, MarketSide::Down))
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
        let stat = DownCaptureRatio::new(None);
        assert_eq!(stat.name(), "Down Capture Ratio (252 days)");
    }

    #[rstest]
    fn test_name_non_default_period() {
        let stat = DownCaptureRatio::new(Some(63));
        assert_eq!(stat.name(), "Down Capture Ratio (63 days)");
    }

    #[rstest]
    fn test_known_value_small_period() {
        // Small period = 4 keeps the geometric annualization hand-checkable.
        //   b = [0.01, -0.02, 0.015, -0.005], r = [0.02, -0.04, 0.030, -0.010]
        // down subset (b < 0) is days 1,3: b_dn = [-0.02, -0.005],
        // r_dn = [-0.04, -0.010], m = 2, period = 4.
        //   annual_r = (0.96*0.99)^(4/2) - 1 = (0.9504)^2 - 1 = -0.09673984
        //   annual_b = (0.98*0.995)^(4/2) - 1 = (0.9751)^2 - 1 = -0.04917999
        //   down_capture = annual_r / annual_b = 1.967056927014422
        // Cross-validated against empyrical 0.5.5 down_capture (ann_factor=4).
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010]);
        let stat = DownCaptureRatio::new(Some(4));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(
            f64,
            result,
            1.967_056_927_014_422,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_known_value_default_period() {
        // Default period = 252; same down subset as above but exercises the 252 path.
        //   r_dn = [-0.04, -0.010], b_dn = [-0.02, -0.005], m = 2, period = 252.
        //   annual_r = (0.96*0.99)^(252/2) - 1
        //   annual_b = (0.98*0.995)^(252/2) - 1
        //   down_capture = annual_r / annual_b = 1.0418038205588374
        // Cross-validated against empyrical 0.5.5 down_capture (period='daily').
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010]);
        let stat = DownCaptureRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(
            f64,
            result,
            1.041_803_820_558_837_4,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_no_down_periods_is_nan() {
        // Benchmark never negative -> down subset empty -> NaN.
        let benchmark = create_returns(&[0.01, 0.02, 0.015, 0.005]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010]);
        let stat = DownCaptureRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_partial_overlap_inner_join() {
        // Strategy on days 0..5, benchmark on days 2..7 -> overlap on days 2,3,4 only.
        let one_day = 86_400_000_000_000_u64;
        let start = 1_600_000_000_000_000_000_u64;

        let mut returns = BTreeMap::new();
        for (i, v) in [0.02, -0.04, 0.030, -0.010, 0.050].iter().enumerate() {
            returns.insert(UnixNanos::from(start + i as u64 * one_day), *v);
        }
        let mut benchmark = BTreeMap::new();
        for (i, v) in [0.015, -0.005, 0.025, -0.02, 0.01].iter().enumerate() {
            benchmark.insert(UnixNanos::from(start + (i as u64 + 2) * one_day), *v);
        }

        // Overlap days 2,3,4: r = [0.030, -0.010, 0.050], b = [0.015, -0.005, 0.025].
        // down subset (b < 0) is day 3 only: r_dn = [-0.010], b_dn = [-0.005],
        // m = 1, period = 252.
        //   annual_r = (0.99)^(252/1) - 1 = -0.9205545483094462
        //   annual_b = (0.995)^(252/1) - 1 = -0.7172410580445943
        //   down_capture = annual_r / annual_b = 1.2834660508966722
        // Cross-validated against empyrical 0.5.5 down_capture on the subset (period='daily').
        let stat = DownCaptureRatio::new(Some(252));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(
            f64,
            result,
            1.283_466_050_896_672_2,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_empty_returns_is_nan() {
        let stat = DownCaptureRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&create_returns(&[]), &create_returns(&[]))
            .unwrap();
        assert!(result.is_nan());
    }

    #[rstest]
    fn test_single_overlap_is_nan() {
        let benchmark = create_returns(&[0.01, -0.02, 0.015]);
        let returns = create_returns(&[0.02]);
        let stat = DownCaptureRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }
}
