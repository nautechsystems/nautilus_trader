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

use crate::{Returns, statistic::PortfolioStatistic};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct ReturnsVolatility {
    period: usize,
}

impl ReturnsVolatility {
    /// Creates a new [`ReturnsVolatility`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl PortfolioStatistic for ReturnsVolatility {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(ReturnsVolatility).to_string()
    }

    fn calculate_from_returns(&self, raw_returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(raw_returns) {
            return Some(f64::NAN);
        }

        let returns = self.downsample_to_daily_bins(raw_returns);
        let daily_std = self.calculate_std(&returns);
        let annualized_std = daily_std * (self.period as f64).sqrt();
        Some(annualized_std)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nautilus_core::UnixNanos;

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

    #[test]
    fn test_empty_returns() {
        let volatility = ReturnsVolatility::new(None);
        let returns = create_returns(vec![]);
        let result = volatility.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[test]
    fn test_default_period() {
        let volatility = ReturnsVolatility::new(None);
        assert_eq!(volatility.period, 252);
    }

    #[test]
    fn test_custom_period() {
        let volatility = ReturnsVolatility::new(Some(365));
        assert_eq!(volatility.period, 365);
    }

    #[test]
    fn test_volatility_calculation() {
        let volatility = ReturnsVolatility::new(None);

        let returns = create_returns(vec![
            0.01, -0.02, 0.03, -0.01, 0.02, 0.04, -0.03, 0.05, -0.04, 0.02,
        ]);
        let result = volatility.calculate_from_returns(&returns);
        assert!(result.is_some());

        assert_eq!(result.unwrap(), 0.48526281538976396);
    }

    #[test]
    fn test_name() {
        let volatility = ReturnsVolatility::new(None);
        assert_eq!(volatility.name(), "ReturnsVolatility");
    }
}
