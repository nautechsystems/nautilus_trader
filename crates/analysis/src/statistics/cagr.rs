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

//! Compound Annual Growth Rate (CAGR) statistic.

use std::collections::BTreeMap;

use nautilus_core::UnixNanos;

use crate::statistic::PortfolioStatistic;

/// Calculates the Compound Annual Growth Rate (CAGR) for returns.
///
/// CAGR represents the mean annual growth rate of an investment over a specified period,
/// assuming the profits were reinvested at the end of each period.
///
/// Formula: CAGR = (Ending Value / Beginning Value)^(Period/Days) - 1
///
/// For returns: CAGR = ((1 + Total Return)^(Period/Days)) - 1
#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct CAGR {
    /// The number of periods per year for annualization (e.g., 252 for trading days).
    pub period: usize,
}

impl CAGR {
    /// Creates a new [`CAGR`] instance.
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl PortfolioStatistic for CAGR {
    type Item = f64;

    fn name(&self) -> String {
        format!("CAGR ({} days)", self.period)
    }

    fn calculate_from_returns(&self, returns: &BTreeMap<UnixNanos, f64>) -> Option<Self::Item> {
        if returns.is_empty() {
            return Some(0.0);
        }

        // Downsample to daily bins to count actual trading days (not calendar days or trade count)
        let daily_returns = self.downsample_to_daily_bins(returns);

        // Calculate total return (cumulative)
        let total_return: f64 = daily_returns.values().map(|&r| 1.0 + r).product::<f64>() - 1.0;

        // Use the number of trading days (bins) for annualization
        // Minimum of 1 day to handle intraday-only strategies
        let days = daily_returns.len().max(1) as f64;

        // CAGR = (1 + total_return)^(period/days) - 1
        let cagr = (1.0 + total_return).powf(self.period as f64 / days) - 1.0;

        if cagr.is_finite() {
            Some(cagr)
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
        let cagr = CAGR::new(Some(252));
        assert_eq!(cagr.name(), "CAGR (252 days)");
    }

    #[rstest]
    fn test_empty_returns() {
        let cagr = CAGR::new(Some(252));
        let returns = BTreeMap::new();
        let result = cagr.calculate_from_returns(&returns);
        assert_eq!(result, Some(0.0));
    }

    #[rstest]
    fn test_positive_cagr() {
        let cagr = CAGR::new(Some(252));
        // Simulate 252 days with 0.1% daily return
        // Total return = (1.001)^252 - 1 ≈ 0.288 (28.8%)
        // CAGR should be approximately same as total return for full year
        let returns = create_returns(vec![0.001; 252]);
        let result = cagr.calculate_from_returns(&returns).unwrap();

        // For 252 days of 0.1% daily return
        // CAGR = (1 + 0.288)^(252/252) - 1 = 0.288
        assert!((result - 0.288).abs() < 0.01);
    }

    #[rstest]
    fn test_cagr_half_year() {
        let cagr = CAGR::new(Some(252));
        // Simulate 126 days (half year) with total return of 10%
        let daily_return = (1.10_f64.powf(1.0 / 126.0)) - 1.0;
        let returns = create_returns(vec![daily_return; 126]);
        let result = cagr.calculate_from_returns(&returns).unwrap();

        // CAGR should annualize the 10% half-year return
        // CAGR = (1.10)^(252/126) - 1 = (1.10)^2 - 1 ≈ 0.21 (21%)
        assert!((result - 0.21).abs() < 0.01);
    }

    #[rstest]
    fn test_negative_returns() {
        let cagr = CAGR::new(Some(252));
        // Simulate losses
        let returns = create_returns(vec![-0.001; 252]);
        let result = cagr.calculate_from_returns(&returns).unwrap();

        // Should be negative
        assert!(result < 0.0);
    }

    #[rstest]
    fn test_multiple_trades_per_day() {
        let cagr = CAGR::new(Some(252));

        // Simulate 500 trades over 252 days
        let mut returns = BTreeMap::new();
        let nanos_per_day = 86_400_000_000_000;
        let start_time = 1_600_000_000_000_000_000;

        // Create 500 trades with small returns spread across 252 days (~2 trades per day)
        for i in 0..500 {
            let day = (i * 252) / 500; // Map trade index to day
            let timestamp =
                start_time + day as u64 * nanos_per_day + (i % 3) as u64 * 1_000_000_000;
            returns.insert(UnixNanos::from(timestamp), 0.0005);
        }

        let result = cagr.calculate_from_returns(&returns).unwrap();

        // With downsample_to_daily_bins, we get 252 bins (trading days)
        // Daily returns are aggregated, then we compound and annualize
        // The CAGR should reflect 252 trading days, NOT 500 trades
        assert!((result - 0.285).abs() < 0.02);
        assert!(result > 0.2); // Should be much higher than what trade-count formula would give
    }

    #[rstest]
    fn test_intraday_trading() {
        let cagr = CAGR::new(Some(252));

        // Simulate multiple trades within a single day
        let mut returns = BTreeMap::new();
        let start_time = 1_600_000_000_000_000_000;

        // 10 trades within the same day, each with 1% return
        for i in 0..10 {
            let timestamp = start_time + i as u64 * 3_600_000_000_000; // 1 hour apart
            returns.insert(UnixNanos::from(timestamp), 0.01);
        }

        let result = cagr.calculate_from_returns(&returns).unwrap();

        // Total return: (1.01)^10 - 1 ≈ 0.1046 (10.46%)
        // This should be treated as 1 trading day
        // Annualized: (1.1046)^(252/1) - 1 = very large number
        // The key is it should NOT return 0.0
        assert!(result > 0.0);
        assert!(result.is_finite());
    }
}
