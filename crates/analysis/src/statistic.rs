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

use std::fmt::Debug;

use nautilus_core::DurationNanos;
use nautilus_model::position::Position;

use crate::Returns;

const IMPL_ERR: &str = "is not implemented for";

/// Trait for portfolio performance statistics that can be calculated from different data sources.
///
/// This trait provides a flexible framework for implementing various financial performance
/// metrics that can operate on returns, realized PnLs, or positions data.
/// Each statistic implementation should override the relevant calculation methods.
///
/// The analyzer calls `calculate_from_returns`, `calculate_from_realized_pnls`, and
/// `calculate_from_positions` on every registered statistic, and their defaults panic, so an
/// implementation must override all three and return `None` for a category it does not support.
/// `calculate_from_returns_with_benchmark` defaults to `None` and is optional.
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

    /// Calculates the statistic from position data.
    ///
    /// # Panics
    ///
    /// Panics if this method is not implemented for the specific statistic.
    fn calculate_from_positions(&self, positions: &[Position]) -> Option<Self::Item> {
        panic!("`calculate_from_positions` {IMPL_ERR} `{}`", self.name());
    }

    /// Calculates the statistic from time-indexed strategy returns relative to a benchmark.
    ///
    /// Defaults to `None`; only benchmark-relative statistics (beta, alpha, information
    /// ratio, tracking error, Treynor ratio) override this method. The `None` default
    /// lets analyzer loops filter results by `Option` - non-benchmark statistics are
    /// simply skipped, as `get_performance_stats_general` already does with
    /// `calculate_from_positions` results - rather than panicking.
    fn calculate_from_returns_with_benchmark(
        &self,
        returns: &Returns,
        benchmark: &Returns,
    ) -> Option<Self::Item> {
        None
    }

    /// Aligns two returns series onto a common daily grid.
    ///
    /// Both series are first downsampled to daily bins (geometric compounding within each
    /// UTC day), then inner-joined on shared timestamps. Timestamps present in only one
    /// series are dropped (not zero-filled). Returns the aligned `(strategy, benchmark)`
    /// value vectors, in ascending timestamp order.
    fn align_returns(&self, a: &Returns, b: &Returns) -> (Vec<f64>, Vec<f64>) {
        let a_daily = self.downsample_to_daily_bins(a);
        let b_daily = self.downsample_to_daily_bins(b);

        let mut aligned_a = Vec::new();
        let mut aligned_b = Vec::new();

        for (timestamp, &a_value) in &a_daily {
            if let Some(&b_value) = b_daily.get(timestamp) {
                aligned_a.push(a_value);
                aligned_b.push(b_value);
            }
        }

        (aligned_a, aligned_b)
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
        let day = DurationNanos::from_days(1);
        let mut daily_bins = Returns::new();

        for (&timestamp, &value) in returns {
            let day_start = timestamp.floor(day);

            daily_bins
                .entry(day_start)
                .and_modify(|entry| {
                    if *entry == 0.0 {
                        *entry = value;
                    } else if value != 0.0 {
                        let previous = *entry;
                        *entry = if previous.abs() <= 0.5 && value.abs() <= 0.5 {
                            // Use a + b + ab to avoid cancellation after adding 1.
                            previous.mul_add(value, previous + value)
                        } else {
                            (1.0 + previous).mul_add(1.0 + value, -1.0)
                        };
                    }
                })
                .or_insert(value);
        }

        daily_bins
    }

    /// Calculates the standard deviation of returns with Bessel's correction.
    fn calculate_std(&self, returns: &Returns) -> f64 {
        mean_and_sample_std(returns.values().copied()).1
    }
}

#[expect(clippy::float_cmp, reason = "detects exactly constant series")]
pub(crate) fn mean_and_sample_std(
    values: impl ExactSizeIterator<Item = f64> + Clone,
) -> (f64, f64) {
    let n = values.len() as f64;
    let first = values.clone().next().unwrap_or(f64::NAN);
    if n < 2.0 {
        return (first, f64::NAN);
    }

    // Summing identical values can round the mean away from the observations
    if first.is_finite() && values.clone().all(|value| value == first) {
        return (first, 0.0);
    }

    let mean = values.clone().sum::<f64>() / n;
    let variance = values.map(|value| (value - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, variance.sqrt())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nautilus_core::{UnixNanos, approx_eq};
    use rstest::rstest;

    use super::*;
    use crate::statistics::{
        returns_kurtosis::ReturnsKurtosis, returns_skewness::ReturnsSkewness,
        returns_volatility::ReturnsVolatility, value_at_risk::ValueAtRisk,
    };

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
        let stat = DummyStat;
        let mut returns: Returns = BTreeMap::new();
        returns.insert(UnixNanos::from(BASE_NS), 1e-17);
        returns.insert(UnixNanos::from(BASE_NS + NANOS_PER_DAY), -0.02);
        returns.insert(UnixNanos::from(BASE_NS + 2 * NANOS_PER_DAY), -0.0);

        let daily = stat.downsample_to_daily_bins(&returns);

        let day = DurationNanos::from_days(1);
        let expected = returns
            .iter()
            .map(|(&timestamp, &value)| (timestamp.floor(day), value.to_bits()))
            .collect::<BTreeMap<_, _>>();
        let actual = daily
            .iter()
            .map(|(&timestamp, &value)| (timestamp, value.to_bits()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_downsample_breakeven_observations_preserve_daily_return() {
        let stat = DummyStat;
        let returns = Returns::from([
            (UnixNanos::from(1), 0.1),
            (UnixNanos::from(NANOS_PER_DAY + 1), 0.1),
            (UnixNanos::from(NANOS_PER_DAY + 2), 0.0),
            (UnixNanos::from(2 * NANOS_PER_DAY + 1), 0.0),
            (UnixNanos::from(2 * NANOS_PER_DAY + 2), 0.1),
        ]);

        let daily = stat.downsample_to_daily_bins(&returns);

        assert_eq!(
            daily,
            Returns::from([
                (UnixNanos::from(0), 0.1),
                (UnixNanos::from(NANOS_PER_DAY), 0.1),
                (UnixNanos::from(2 * NANOS_PER_DAY), 0.1),
            ]),
        );
    }

    #[rstest]
    fn test_downsample_preserves_sign_of_nearly_offsetting_returns() {
        let small = 2.0_f64.powi(-30);
        let residual = 2.0_f64.powi(-53);
        let first = small + residual;
        // These binary powers give the exact compounded return in binary64.
        let expected = residual - first * small;
        let returns = Returns::from([
            (UnixNanos::from(BASE_NS), first),
            (UnixNanos::from(BASE_NS + 1), -small),
        ]);

        let daily = DummyStat.downsample_to_daily_bins(&returns);
        let value = *daily.values().next().unwrap();
        let var = ValueAtRisk::new(None)
            .calculate_from_returns(&returns)
            .unwrap();
        assert!(expected > 0.0);
        assert_eq!(value.to_bits(), expected.to_bits());
        assert_eq!(var.to_bits(), expected.to_bits());
    }

    #[rstest]
    fn test_downsample_preserves_large_return_when_zero_is_appended() {
        let single = Returns::from([(UnixNanos::from(BASE_NS), 1.3)]);
        let with_zero = Returns::from([
            (UnixNanos::from(BASE_NS), 1.3),
            (UnixNanos::from(BASE_NS + 1), 0.0),
        ]);

        let single_daily = DummyStat.downsample_to_daily_bins(&single);
        let with_zero_daily = DummyStat.downsample_to_daily_bins(&with_zero);
        let expected = 1.3_f64.to_bits();
        assert_eq!(single_daily.values().next().unwrap().to_bits(), expected);
        assert_eq!(with_zero_daily.values().next().unwrap().to_bits(), expected);
    }

    #[rstest]
    fn test_downsample_preserves_utc_boundary_and_total_loss() {
        let returns = Returns::from([
            (UnixNanos::from(NANOS_PER_DAY - 2), 0.125),
            (UnixNanos::from(NANOS_PER_DAY - 1), -0.25),
            (UnixNanos::from(NANOS_PER_DAY), -1.0),
            (UnixNanos::from(NANOS_PER_DAY + 1), 1.0e20),
        ]);

        let daily = DummyStat.downsample_to_daily_bins(&returns);
        assert_eq!(daily.len(), 2);
        assert_eq!(
            daily[&UnixNanos::from(0)].to_bits(),
            (-0.15625_f64).to_bits()
        );
        assert_eq!(
            daily[&UnixNanos::from(NANOS_PER_DAY)].to_bits(),
            (-1.0_f64).to_bits()
        );
    }

    #[rstest]
    fn test_constant_daily_returns_have_zero_volatility_and_undefined_moments() {
        let returns = (0..10)
            .map(|day| (UnixNanos::from(BASE_NS + day * NANOS_PER_DAY), 0.01))
            .collect::<Returns>();

        let volatility = ReturnsVolatility::new(None)
            .calculate_from_returns(&returns)
            .unwrap();
        let skewness = ReturnsSkewness::new()
            .calculate_from_returns(&returns)
            .unwrap();
        let kurtosis = ReturnsKurtosis::new()
            .calculate_from_returns(&returns)
            .unwrap();
        assert_eq!(volatility.to_bits(), 0.0_f64.to_bits());
        assert!(skewness.is_nan());
        assert!(kurtosis.is_nan());
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
