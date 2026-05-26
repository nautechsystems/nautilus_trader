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

use std::{collections::BTreeMap, fmt::Debug};

use nautilus_model::{orders::Order, position::Position};

use crate::Returns;

const IMPL_ERR: &str = "is not implemented for";

/// Trait for portfolio performance statistics that can be calculated from different data sources.
///
/// This trait provides a flexible framework for implementing various financial performance
/// metrics that can operate on returns, realized PnLs, orders, or positions data.
/// Each statistic implementation should override the relevant calculation methods.
#[allow(unused_variables)]
pub trait PortfolioStatistic: Debug {
    type Item;

    /// Returns the name of this statistic for display and identification purposes.
    fn name(&self) -> String;

    /// Calculates the statistic from time-indexed returns data.
    ///
    /// # Panics
    ///
    /// Panics if this method is not implemented for the specific statistic.
    fn calculate_from_returns(&self, returns: &Returns) -> Option<Self::Item> {
        panic!("`calculate_from_returns` {IMPL_ERR} `{}`", self.name());
    }

    /// Calculates the statistic from realized profit and loss values.
    ///
    /// # Panics
    ///
    /// Panics if this method is not implemented for the specific statistic.
    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        panic!(
            "`calculate_from_realized_pnls` {IMPL_ERR} `{}`",
            self.name()
        );
    }

    /// Calculates the statistic from order data.
    ///
    /// # Panics
    ///
    /// Panics if this method is not implemented for the specific statistic.
    #[allow(dead_code)]
    fn calculate_from_orders(&self, orders: Vec<Box<dyn Order>>) -> Option<Self::Item> {
        panic!("`calculate_from_orders` {IMPL_ERR} `{}`", self.name());
    }

    /// Calculates the statistic from position data.
    ///
    /// # Panics
    ///
    /// Panics if this method is not implemented for the specific statistic.
    fn calculate_from_positions(&self, positions: &[Position]) -> Option<Self::Item> {
        panic!("`calculate_from_positions` {IMPL_ERR} `{}`", self.name());
    }

    /// Validates that returns data is not empty.
    fn check_valid_returns(&self, returns: &Returns) -> bool {
        !returns.is_empty()
    }

    /// Downsamples high-frequency returns to daily bins by geometric compounding.
    ///
    /// Within each UTC day, returns are combined via `(1 + r1)(1 + r2) - 1` to produce
    /// the day's effective return, which is the standard convention for chaining
    /// arithmetic period returns. For daily-frequency inputs (one return per day) the
    /// bin value is identical to the input value, so callers that already operate on
    /// daily returns observe no behavior change.
    fn downsample_to_daily_bins(&self, returns: &Returns) -> Returns {
        let nanos_per_day = 86_400_000_000_000; // Number of nanoseconds in a day
        let mut daily_bins = BTreeMap::new();

        for (&timestamp, &value) in returns {
            // Calculate the start of the day in nanoseconds for the given timestamp
            let day_start = timestamp - (timestamp.as_u64() % nanos_per_day);

            // Geometrically compound returns within each day
            let entry = daily_bins.entry(day_start).or_insert(0.0_f64);
            *entry = (1.0_f64 + *entry).mul_add(1.0_f64 + value, -1.0_f64);
        }

        daily_bins
    }

    /// Calculates the standard deviation of returns with Bessel's correction.
    fn calculate_std(&self, returns: &Returns) -> f64 {
        let n = returns.len() as f64;
        if n < 2.0 {
            return f64::NAN;
        }

        let mean = returns.values().sum::<f64>() / n;

        let variance = returns.values().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);

        variance.sqrt()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UnixNanos, approx_eq};
    use rstest::rstest;

    use super::*;

    #[derive(Debug)]
    struct DummyStat;

    impl PortfolioStatistic for DummyStat {
        type Item = f64;

        fn name(&self) -> String {
            "DummyStat".to_string()
        }
    }

    const NANOS_PER_DAY: u64 = 86_400_000_000_000;
    const BASE_NS: u64 = 1_600_000_000_000_000_000;

    #[rstest]
    fn test_downsample_compounds_intraday_returns() {
        // Two intraday returns in the same UTC day: +5% then -5%.
        //   arithmetic sum:  0.05 + (-0.05) = 0.00      (incorrect)
        //   geometric chain: (1.05)(0.95) - 1 = -0.0025 (correct)
        let stat = DummyStat;
        let mut returns: Returns = BTreeMap::new();
        returns.insert(UnixNanos::from(BASE_NS), 0.05);
        returns.insert(UnixNanos::from(BASE_NS + 3_600_000_000_000), -0.05);

        let daily = stat.downsample_to_daily_bins(&returns);

        assert_eq!(daily.len(), 1);
        let value = *daily.values().next().unwrap();
        assert!(approx_eq!(f64, value, -0.0025, epsilon = 1e-12));
    }

    #[rstest]
    fn test_downsample_daily_inputs_unchanged() {
        // For one-return-per-day inputs the bin value equals the input return,
        // so existing callers that already pass daily returns see no change.
        let stat = DummyStat;
        let mut returns: Returns = BTreeMap::new();
        returns.insert(UnixNanos::from(BASE_NS), 0.01);
        returns.insert(UnixNanos::from(BASE_NS + NANOS_PER_DAY), -0.02);
        returns.insert(UnixNanos::from(BASE_NS + 2 * NANOS_PER_DAY), 0.015);

        let daily = stat.downsample_to_daily_bins(&returns);

        let values: Vec<f64> = daily.values().copied().collect();
        assert_eq!(values.len(), 3);
        assert!(approx_eq!(f64, values[0], 0.01, epsilon = 1e-15));
        assert!(approx_eq!(f64, values[1], -0.02, epsilon = 1e-15));
        assert!(approx_eq!(f64, values[2], 0.015, epsilon = 1e-15));
    }

    #[rstest]
    fn test_downsample_chains_three_intraday_returns() {
        // Three returns in the same day: +1%, +2%, -1%.
        //   geometric chain: (1.01)(1.02)(0.99) - 1 = 0.019998
        let stat = DummyStat;
        let mut returns: Returns = BTreeMap::new();
        returns.insert(UnixNanos::from(BASE_NS), 0.01);
        returns.insert(UnixNanos::from(BASE_NS + 3_600_000_000_000), 0.02);
        returns.insert(UnixNanos::from(BASE_NS + 7_200_000_000_000), -0.01);

        let daily = stat.downsample_to_daily_bins(&returns);

        assert_eq!(daily.len(), 1);
        let value = *daily.values().next().unwrap();
        let expected = 1.01_f64 * 1.02 * 0.99 - 1.0;
        assert!(approx_eq!(f64, value, expected, epsilon = 1e-12));
    }
}
