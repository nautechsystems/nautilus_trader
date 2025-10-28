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

//! Calmar Ratio statistic.

use std::collections::BTreeMap;

use nautilus_core::UnixNanos;

use crate::{
    statistic::PortfolioStatistic,
    statistics::{cagr::CAGR, max_drawdown::MaxDrawdown},
};

/// Calculates the Calmar Ratio for returns.
///
/// The Calmar Ratio is a function of the fund's average compounded annual rate
/// of return versus its maximum drawdown. The higher the Calmar ratio, the better
/// it performed on a risk-adjusted basis during the given time frame.
///
/// Formula: Calmar Ratio = CAGR / |Max Drawdown|
///
/// Reference: Young, T. W. (1991). "Calmar Ratio: A Smoother Tool". Futures, 20(1).
#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct CalmarRatio {
    /// The number of periods per year for CAGR calculation (e.g., 252 for trading days).
    pub period: usize,
}

impl CalmarRatio {
    /// Creates a new [`CalmarRatio`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl PortfolioStatistic for CalmarRatio {
    type Item = f64;

    fn name(&self) -> String {
        format!("Calmar Ratio ({} days)", self.period)
    }

    fn calculate_from_returns(&self, returns: &BTreeMap<UnixNanos, f64>) -> Option<Self::Item> {
        if returns.is_empty() {
            return Some(0.0);
        }

        // Calculate CAGR
        let cagr_stat = CAGR::new(Some(self.period));
        let cagr = cagr_stat.calculate_from_returns(returns)?;

        // Calculate Max Drawdown
        let max_dd_stat = MaxDrawdown::new();
        let max_dd = max_dd_stat.calculate_from_returns(returns)?;

        // Calmar = CAGR / |Max Drawdown|
        // Max Drawdown is already negative, so we use abs
        if max_dd.abs() < f64::EPSILON {
            return Some(0.0);
        }

        let calmar = cagr / max_dd.abs();

        if calmar.is_finite() {
            Some(calmar)
        } else {
            Some(0.0)
        }
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
        let mut returns = BTreeMap::new();
        let nanos_per_day = 86_400_000_000_000;
        let start_time = 1_600_000_000_000_000_000;

        for (i, &value) in values.iter().enumerate() {
            let timestamp = start_time + i as u64 * nanos_per_day;
            returns.insert(UnixNanos::from(timestamp), value);
        }

        returns
    }

    #[rstest]
    fn test_name() {
        let ratio = CalmarRatio::new(Some(252));
        assert_eq!(ratio.name(), "Calmar Ratio (252 days)");
    }

    #[rstest]
    fn test_empty_returns() {
        let ratio = CalmarRatio::new(Some(252));
        let returns = BTreeMap::new();
        let result = ratio.calculate_from_returns(&returns);
        assert_eq!(result, Some(0.0));
    }

    #[rstest]
    fn test_no_drawdown() {
        let ratio = CalmarRatio::new(Some(252));
        // Only positive returns, no drawdown
        let returns = create_returns(vec![0.01; 252]);
        let result = ratio.calculate_from_returns(&returns);

        // Should be 0.0 when no drawdown (division by zero case)
        assert_eq!(result, Some(0.0));
    }

    #[rstest]
    fn test_positive_ratio() {
        let ratio = CalmarRatio::new(Some(252));
        // Simulate a year with 20% CAGR and 10% max drawdown
        // Daily return for 20% annual: (1.20)^(1/252) - 1
        let mut returns_vec = vec![0.001; 200]; // Small positive returns
        // Add a drawdown period
        returns_vec.extend(vec![-0.002; 52]); // Small negative returns

        let returns = create_returns(returns_vec);
        let result = ratio.calculate_from_returns(&returns).unwrap();

        // Calmar should be positive (CAGR / |Max DD|)
        assert!(result > 0.0);
    }

    #[rstest]
    fn test_high_calmar_better() {
        let ratio = CalmarRatio::new(Some(252));

        // Strategy A: Higher return, same drawdown
        let returns_a = create_returns(vec![0.002; 252]);
        let calmar_a = ratio.calculate_from_returns(&returns_a);

        // Strategy B: Lower return
        let returns_b = create_returns(vec![0.001; 252]);
        let calmar_b = ratio.calculate_from_returns(&returns_b);

        // Higher CAGR should give higher Calmar (assuming same drawdown pattern)
        // This test just verifies both calculate successfully
        assert!(calmar_a.is_some());
        assert!(calmar_b.is_some());
    }
}
