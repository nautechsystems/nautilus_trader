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

//! Ulcer Index statistic.

use std::collections::BTreeMap;

use nautilus_core::UnixNanos;
use nautilus_model::position::Position;

use crate::statistic::PortfolioStatistic;

/// Calculates the Ulcer Index of portfolio returns.
///
/// The Ulcer Index measures downside risk as the root-mean-square of the
/// percentage drawdowns of the cumulative-return equity curve. Unlike volatility
/// it only penalizes downside deviations, and unlike maximum drawdown it accounts
/// for both the depth and the duration of drawdowns.
///
/// The equity curve compounds returns from a starting value of `1.0`, and each
/// drawdown is measured against the running peak (matching the convention used by
/// [`MaxDrawdown`](super::max_drawdown::MaxDrawdown)):
///
/// `UI = sqrt( mean( D_i^2 ) )`, where `D_i = (peak_i - equity_i) / peak_i`
///
/// Drawdowns are expressed as fractions (`0.05` = 5%), so the result is on the
/// same scale as `MaxDrawdown`. Returns `0.0` for an empty series.
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
pub struct UlcerIndex {}

impl UlcerIndex {
    /// Creates a new [`UlcerIndex`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

impl PortfolioStatistic for UlcerIndex {
    type Item = f64;

    fn name(&self) -> String {
        "Ulcer Index".to_string()
    }

    fn calculate_from_returns(&self, returns: &BTreeMap<UnixNanos, f64>) -> Option<Self::Item> {
        if returns.is_empty() {
            return Some(0.0);
        }

        // Compound returns into an equity curve starting from 1.0 and accumulate
        // the squared percentage drawdown from the running peak at each step.
        let mut cumulative = 1.0;
        let mut running_max = 1.0;
        let mut sum_squared_drawdown = 0.0;

        for &ret in returns.values() {
            cumulative *= 1.0 + ret;

            if cumulative > running_max {
                running_max = cumulative;
            }

            let drawdown = (running_max - cumulative) / running_max;
            sum_squared_drawdown += drawdown * drawdown;
        }

        Some((sum_squared_drawdown / returns.len() as f64).sqrt())
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
    use nautilus_core::approx_eq;
    use rstest::rstest;

    use super::*;

    fn create_returns(values: &[f64]) -> BTreeMap<UnixNanos, f64> {
        values
            .iter()
            .copied()
            .enumerate()
            .map(|(i, v)| (UnixNanos::from(i as u64), v))
            .collect()
    }

    #[rstest]
    fn test_name() {
        let stat = UlcerIndex::new();
        assert_eq!(stat.name(), "Ulcer Index");
    }

    #[rstest]
    fn test_empty_returns() {
        let stat = UlcerIndex::new();
        let returns = BTreeMap::new();
        assert_eq!(stat.calculate_from_returns(&returns), Some(0.0));
    }

    #[rstest]
    fn test_no_drawdown_is_zero() {
        // Monotonically rising equity has no drawdown, so the Ulcer Index is 0.
        let stat = UlcerIndex::new();
        let returns = create_returns(&[0.01, 0.02, 0.01, 0.015]);
        let result = stat.calculate_from_returns(&returns).unwrap();
        assert!(approx_eq!(f64, result, 0.0, epsilon = 1e-12));
    }

    #[rstest]
    fn test_ulcer_index_calculation() {
        // Reference value cross-checked against numpy (see PR description):
        //   equity = cumprod(1 + r), dd = (peak - equity) / peak,
        //   UI = sqrt(mean(dd^2)) with the 1.0 starting-capital baseline.
        let stat = UlcerIndex::new();
        let returns = create_returns(&[0.10, -0.10, 0.50, -0.20, 0.10]);
        let result = stat.calculate_from_returns(&returns).unwrap();
        assert!(approx_eq!(
            f64,
            result,
            0.11349008767288883,
            epsilon = 1e-12
        ));
    }
}
