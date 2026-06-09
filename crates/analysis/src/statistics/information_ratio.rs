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

use crate::{Returns, statistic::PortfolioStatistic};

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
        let n = r.len();
        if n < 2 {
            return Some(f64::NAN);
        }

        let active: Vec<f64> = r.iter().zip(b.iter()).map(|(&ri, &bi)| ri - bi).collect();
        let nf = n as f64;
        let mean_active = active.iter().sum::<f64>() / nf;
        let variance = active
            .iter()
            .map(|&x| (x - mean_active).powi(2))
            .sum::<f64>()
            / (nf - 1.0);
        let std_active = variance.sqrt();

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
