// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Maximum Drawdown statistic.

use std::collections::BTreeMap;

use nautilus_core::UnixNanos;

use crate::statistic::PortfolioStatistic;

/// Calculates the Maximum Drawdown for returns.
///
/// Maximum Drawdown is the maximum observed loss from a peak to a trough,
/// before a new peak is attained. It is an indicator of downside risk over
/// a specified time period.
///
/// Formula: Max((Peak - Trough) / Peak) for all peak-trough sequences
#[repr(C)]
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct MaxDrawdown {}

impl MaxDrawdown {
    /// Creates a new [`MaxDrawdown`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

impl PortfolioStatistic for MaxDrawdown {
    type Item = f64;

    fn name(&self) -> String {
        "Max Drawdown".to_string()
    }

    fn calculate_from_returns(&self, returns: &BTreeMap<UnixNanos, f64>) -> Option<Self::Item> {
        if returns.is_empty() {
            return Some(0.0);
        }

        // Calculate cumulative returns starting from 1.0
        let mut cumulative = 1.0;
        let mut running_max = 1.0;
        let mut max_drawdown = 0.0;

        for &ret in returns.values() {
            cumulative *= 1.0 + ret;

            // Update running maximum
            if cumulative > running_max {
                running_max = cumulative;
            }

            // Calculate drawdown from running max
            let drawdown = (running_max - cumulative) / running_max;

            // Update maximum drawdown
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }

        // Return as negative percentage
        Some(-max_drawdown)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn create_returns(values: Vec<f64>) -> BTreeMap<UnixNanos, f64> {
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| (UnixNanos::from(i as u64), v))
            .collect()
    }

    #[rstest]
    fn test_name() {
        let stat = MaxDrawdown::new();
        assert_eq!(stat.name(), "Max Drawdown");
    }

    #[rstest]
    fn test_empty_returns() {
        let stat = MaxDrawdown::new();
        let returns = BTreeMap::new();
        let result = stat.calculate_from_returns(&returns);
        assert_eq!(result, Some(0.0));
    }

    #[rstest]
    fn test_no_drawdown() {
        let stat = MaxDrawdown::new();
        // Only positive returns, no drawdown
        let returns = create_returns(vec![0.01, 0.02, 0.01, 0.015]);
        let result = stat.calculate_from_returns(&returns).unwrap();
        assert_eq!(result, 0.0);
    }

    #[rstest]
    fn test_simple_drawdown() {
        let stat = MaxDrawdown::new();
        // Start at 1.0, go to 1.1 (+10%), then drop to 0.99 (-10% from peak)
        // Max DD = (1.1 - 0.99) / 1.1 = 0.1 / 1.1 = 0.0909 (9.09%)
        let returns = create_returns(vec![0.10, -0.10]);
        let result = stat.calculate_from_returns(&returns).unwrap();

        // Should be approximately -0.10 (reported as negative)
        assert!((result + 0.10).abs() < 0.01);
    }

    #[rstest]
    fn test_multiple_drawdowns() {
        let stat = MaxDrawdown::new();
        // Peak at 1.5, trough at 1.0
        // DD1: 10% from 1.0
        // DD2: 20% from 1.5
        let returns = create_returns(vec![0.10, -0.10, 0.50, -0.20, 0.10]);
        let result = stat.calculate_from_returns(&returns).unwrap();

        // Max DD should be the larger one (20%)
        assert!((result + 0.20).abs() < 0.01);
    }

    #[rstest]
    fn test_initial_loss() {
        let stat = MaxDrawdown::new();
        // Start with 40% loss
        let returns = create_returns(vec![-0.40, -0.10]);
        let result = stat.calculate_from_returns(&returns).unwrap();

        // From 1.0 -> 0.6 -> 0.54
        // Max DD from initial 1.0 is 46%
        assert!((result + 0.46).abs() < 0.01);
    }
}
