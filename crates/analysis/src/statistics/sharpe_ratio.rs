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

use std::fmt::{self, Display};

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the Sharpe ratio for portfolio returns.
///
/// The Sharpe ratio measures risk-adjusted return and is calculated as:
/// `(Mean Return - Risk-free Rate) / Standard Deviation of Returns * sqrt(period)`
///
/// This implementation assumes a risk-free rate of 0 and annualizes the ratio
/// using the square root of the specified period (default: 252 trading days).
///
/// # References
///
/// - Sharpe, W. F. (1966). "Mutual Fund Performance". *Journal of Business*, 39(1), 119-138.
/// - Sharpe, W. F. (1994). "The Sharpe Ratio". *Journal of Portfolio Management*, 21(1), 49-58.
/// - CFA Institute Investment Foundations, 3rd Edition
#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct SharpeRatio {
    /// The annualization period (default: 252 for daily data).
    period: usize,
}

impl SharpeRatio {
    /// Creates a new [`SharpeRatio`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl Display for SharpeRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sharpe Ratio ({} days)", self.period)
    }
}

impl PortfolioStatistic for SharpeRatio {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_returns(&self, raw_returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(raw_returns) {
            return Some(f64::NAN);
        }

        let returns = self.downsample_to_daily_bins(raw_returns);
        let mean = returns.values().sum::<f64>() / returns.len() as f64;
        let std = self.calculate_std(&returns);

        if std < f64::EPSILON {
            return Some(f64::NAN);
        }

        let annualized_ratio = (mean / std) * (self.period as f64).sqrt();

        Some(annualized_ratio)
    }
    fn calculate_from_realized_pnls(&self, _realized_pnls: &[f64]) -> Option<Self::Item> {
        None
    }

    fn calculate_from_positions(&self, _positions: &[Position]) -> Option<Self::Item> {
        None
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nautilus_core::{UnixNanos, approx_eq};
    use rstest::rstest;

    use super::*;

    fn create_returns(values: Vec<f64>) -> BTreeMap<UnixNanos, f64> {
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
    fn test_empty_returns() {
        let ratio = SharpeRatio::new(None);
        let returns = create_returns(vec![]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_zero_std_dev() {
        let ratio = SharpeRatio::new(None);
        let returns = create_returns(vec![0.01; 10]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_valid_sharpe_ratio() {
        let ratio = SharpeRatio::new(Some(252));
        let returns = create_returns(vec![0.01, -0.02, 0.015, -0.005, 0.025]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(approx_eq!(
            f64,
            result.unwrap(),
            4.48998886412873,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_name() {
        let ratio = SharpeRatio::new(None);
        assert_eq!(ratio.name(), "Sharpe Ratio (252 days)");
    }
}
