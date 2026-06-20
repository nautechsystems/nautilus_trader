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

//! Up capture ratio statistic (benchmark-relative).

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the up capture ratio of portfolio returns relative to a benchmark.
///
/// The up capture ratio measures how the portfolio performed, on average, during the
/// periods when the benchmark return was positive. It is the ratio of the portfolio's
/// geometric annualized return to the benchmark's geometric annualized return, both
/// computed over the up-market subset only:
///
/// `UpCapture = annualized_return(portfolio | benchmark > 0) / annualized_return(benchmark | benchmark > 0)`
///
/// where each side's annualized return is the geometric (CAGR-style) value
/// `(prod(1 + x_i))^(period / m) - 1` and `m` is the number of up-market periods (the
/// size of the filtered subset, not the full aligned length). The period defaults to
/// 252 trading days. A value above 1.0 means the portfolio outperformed the benchmark
/// in up markets.
///
/// This is the `empyrical.up_capture` convention (geometric annualized-return ratio over
/// the `benchmark > 0` subset). Note that this differs from the Morningstar definition,
/// which uses a ratio of *cumulative* (non-annualized) returns; the two coincide only
/// when both subsets contain the same number of periods.
///
/// # References
///
/// - empyrical `up_capture` / `capture` / `annual_return`
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
pub struct UpCaptureRatio {
    /// The annualization period (default: 252 for daily data).
    period: usize,
}

impl UpCaptureRatio {
    /// Creates a new [`UpCaptureRatio`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl Display for UpCaptureRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Up Capture Ratio ({} days)", self.period)
    }
}

impl PortfolioStatistic for UpCaptureRatio {
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

        Some(capture_ratio(&r, &b, self.period, MarketSide::Up))
    }
}

/// The side of the market (sign of the benchmark return) to filter on.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MarketSide {
    /// Periods where the benchmark return is strictly positive.
    Up,
    /// Periods where the benchmark return is strictly negative.
    Down,
}

/// Computes the geometric (CAGR-style) annualized return of `x`.
///
/// `annualized = (prod(1 + x_i))^(period / m) - 1`, where `m = x.len()`. This mirrors
/// `empyrical.annual_return`, which annualizes by the number of periods in the slice
/// passed to it. Returns `f64::NAN` for an empty slice.
pub(crate) fn geometric_annualized_return(x: &[f64], period: usize) -> f64 {
    let m = x.len();
    if m == 0 {
        return f64::NAN;
    }
    let growth = x.iter().map(|&xi| 1.0 + xi).product::<f64>();
    growth.powf(period as f64 / m as f64) - 1.0
}

/// Computes the capture ratio of `r` against `b` on the requested market side.
///
/// Filters both series to the periods where the benchmark return matches `side`
/// (`b_i > 0` for [`MarketSide::Up`], `b_i < 0` for [`MarketSide::Down`]), then returns
/// the ratio of the portfolio's geometric annualized return to the benchmark's geometric
/// annualized return over that subset, matching the `empyrical.up_capture` /
/// `empyrical.down_capture` convention.
///
/// Returns `f64::NAN` when the filtered subset is empty (no qualifying periods) or when
/// the benchmark's annualized return over the subset is within `f64::EPSILON` of zero
/// (which would otherwise divide by zero). Callers must ensure `r.len() == b.len()`.
pub(crate) fn capture_ratio(r: &[f64], b: &[f64], period: usize, side: MarketSide) -> f64 {
    let mut r_sub = Vec::new();
    let mut b_sub = Vec::new();

    for (&ri, &bi) in r.iter().zip(b.iter()) {
        let keep = match side {
            MarketSide::Up => bi > 0.0,
            MarketSide::Down => bi < 0.0,
        };

        if keep {
            r_sub.push(ri);
            b_sub.push(bi);
        }
    }

    if r_sub.is_empty() {
        return f64::NAN;
    }

    let annual_r = geometric_annualized_return(&r_sub, period);
    let annual_b = geometric_annualized_return(&b_sub, period);
    if annual_b.abs() < f64::EPSILON {
        return f64::NAN;
    }

    annual_r / annual_b
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
        let stat = UpCaptureRatio::new(None);
        assert_eq!(stat.name(), "Up Capture Ratio (252 days)");
    }

    #[rstest]
    fn test_name_non_default_period() {
        let stat = UpCaptureRatio::new(Some(63));
        assert_eq!(stat.name(), "Up Capture Ratio (63 days)");
    }

    #[rstest]
    fn test_known_value() {
        // Default period = 252. With
        //   b = [0.01, -0.02, 0.015, -0.005, 0.025]
        //   r = [0.02, -0.04, 0.030, -0.010, 0.050]
        // the up subset (b > 0) is days 0,2,4: b_up = [0.01, 0.015, 0.025],
        // r_up = [0.02, 0.030, 0.050], m = 3, period = 252.
        //   annual_r = (1.02*1.03*1.05)^(252/3) - 1
        //   annual_b = (1.01*1.015*1.025)^(252/3) - 1
        //   up_capture = annual_r / annual_b
        // Cross-validated against empyrical 0.5.5 up_capture (period='daily'): 60.31258720129805.
        let benchmark = create_returns(&[0.01, -0.02, 0.015, -0.005, 0.025]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010, 0.050]);
        let stat = UpCaptureRatio::new(Some(252));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(
            f64,
            result,
            60.312_587_201_298_05,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_known_value_small_period() {
        // Small period = 4 keeps the geometric annualization hand-checkable.
        //   b = [0.01, -0.02, 0.015, 0.025], r = [0.02, -0.04, 0.030, 0.050]
        // up subset (b > 0) is days 0,2,3: b_up = [0.01, 0.015, 0.025],
        // r_up = [0.02, 0.030, 0.050], m = 3, period = 4.
        //   annual_r = (1.02*1.03*1.05)^(4/3) - 1 = 0.1398182177864391
        //   annual_b = (1.01*1.015*1.025)^(4/3) - 1 = 0.06827166330526313
        //   up_capture = annual_r / annual_b = 2.0479685277516944
        // Cross-validated against empyrical 0.5.5 up_capture (ann_factor=4).
        let benchmark = create_returns(&[0.01, -0.02, 0.015, 0.025]);
        let returns = create_returns(&[0.02, -0.04, 0.030, 0.050]);
        let stat = UpCaptureRatio::new(Some(4));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(
            f64,
            result,
            2.047_968_527_751_694_4,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_no_up_periods_is_nan() {
        // Benchmark never positive -> up subset empty -> NaN.
        let benchmark = create_returns(&[-0.01, -0.02, -0.015, -0.005]);
        let returns = create_returns(&[0.02, -0.04, 0.030, -0.010]);
        let stat = UpCaptureRatio::new(None);
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
        for (i, v) in [0.015, -0.005, 0.025, 0.01, -0.02].iter().enumerate() {
            benchmark.insert(UnixNanos::from(start + (i as u64 + 2) * one_day), *v);
        }

        // Overlap days 2,3,4: r = [0.030, -0.010, 0.050], b = [0.015, -0.005, 0.025].
        // up subset (b > 0) is days 2,4: r_up = [0.030, 0.050], b_up = [0.015, 0.025],
        // m = 2, period = 252.
        //   annual_r = (1.03*1.05)^(252/2) - 1
        //   annual_b = (1.015*1.025)^(252/2) - 1
        //   up_capture = annual_r / annual_b = 133.15737653360318
        // Cross-validated against empyrical 0.5.5 up_capture on the subset (period='daily').
        let stat = UpCaptureRatio::new(Some(252));
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(approx_eq!(
            f64,
            result,
            133.157_376_533_603_18,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_empty_returns_is_nan() {
        let stat = UpCaptureRatio::new(None);
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
        let stat = UpCaptureRatio::new(None);
        let result = stat
            .calculate_from_returns_with_benchmark(&returns, &benchmark)
            .unwrap();
        assert!(result.is_nan());
    }
}
