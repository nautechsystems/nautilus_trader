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

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the risk-return ratio (mean/std) for portfolio returns.
///
/// This is a non-annualized ratio of mean return to standard deviation.
/// For an annualized version, use `SharpeRatio`.
///
/// Downsamples high-frequency returns to daily bins before calculation
/// for consistency with other ratio-based statistics.
#[repr(C)]
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct RiskReturnRatio {}

impl RiskReturnRatio {
    /// Creates a new [`RiskReturnRatio`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

impl Display for RiskReturnRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Risk Return Ratio")
    }
}

impl PortfolioStatistic for RiskReturnRatio {
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

        if std < f64::EPSILON || std.is_nan() {
            Some(f64::NAN)
        } else {
            Some(mean / std)
        }
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

    fn create_returns(values: Vec<f64>) -> Returns {
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
        let ratio = RiskReturnRatio::new();
        let returns = create_returns(vec![]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_zero_std_dev() {
        let ratio = RiskReturnRatio::new();
        let returns = create_returns(vec![0.05; 10]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_valid_risk_return_ratio() {
        let ratio = RiskReturnRatio::new();
        let returns = create_returns(vec![0.1, -0.05, 0.2, -0.1, 0.15]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(approx_eq!(
            f64,
            result.unwrap(),
            0.46360044557175345,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_name() {
        let ratio = RiskReturnRatio::new();
        assert_eq!(ratio.name(), "Risk Return Ratio");
    }
}
