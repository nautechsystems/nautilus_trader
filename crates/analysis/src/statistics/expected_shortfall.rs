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

//! Expected Shortfall (Conditional Value at Risk) statistic.

use std::fmt::Display;

use nautilus_core::correctness::check_predicate_true;
use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic, statistics::value_at_risk::percentile_linear};

/// Calculates the historical Expected Shortfall (Conditional Value at Risk) of
/// portfolio returns.
///
/// Expected Shortfall is the average of the losses that occur beyond the
/// [`ValueAtRisk`](crate::statistics::value_at_risk::ValueAtRisk) threshold at a
/// given confidence level — the mean of the worst
/// `1 - confidence` tail of the return distribution. It is a coherent risk
/// measure and captures tail severity that `VaR` alone does not.
///
/// `ES(c) = mean( r | r <= VaR(c) )`
///
/// `confidence` defaults to `0.95`. The result is expressed as a return (e.g.
/// `-0.05` is a 5% expected tail loss); it is always less than or equal to the
/// corresponding `VaR`. Returns `NaN` for an empty series.
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
pub struct ExpectedShortfall {
    /// The confidence level `c` in `(0, 1)` (default: 0.95).
    confidence: f64,
}

impl ExpectedShortfall {
    /// Creates a new checked [`ExpectedShortfall`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `confidence` is not finite and in the range `(0, 1)`.
    pub fn new_checked(confidence: Option<f64>) -> anyhow::Result<Self> {
        let confidence = confidence.unwrap_or(0.95);
        check_predicate_true(
            confidence.is_finite() && confidence > 0.0 && confidence < 1.0,
            "confidence must be finite and in the range (0, 1)",
        )?;
        Ok(Self { confidence })
    }

    /// Creates a new [`ExpectedShortfall`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `confidence` is not finite and in the range `(0, 1)`.
    #[must_use]
    pub fn new(confidence: Option<f64>) -> Self {
        Self::new_checked(confidence).expect("Invalid `confidence` for `ExpectedShortfall`")
    }
}

impl Display for ExpectedShortfall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expected Shortfall (confidence {})", self.confidence)
    }
}

impl PortfolioStatistic for ExpectedShortfall {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_returns(&self, raw_returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(raw_returns) {
            return Some(f64::NAN);
        }

        // Downsample and sort once; both the VaR threshold and the tail it bounds
        // are taken from this same daily-binned, value-sorted sample, keeping them
        // consistent by construction and avoiding a second downsample + sort.
        let returns = self.downsample_to_daily_bins(raw_returns);
        let mut values: Vec<f64> = returns.values().copied().collect();
        values.sort_by(f64::total_cmp);

        let alpha = 1.0 - self.confidence;
        let var = percentile_linear(&values, alpha * 100.0);
        if var.is_nan() {
            return Some(f64::NAN);
        }

        // The tail is the sorted prefix of returns at or below the VaR threshold.
        // A historical quantile always satisfies `var >= values[0]`, so at least
        // the minimum bin qualifies and the tail is non-empty.
        let cutoff = values.partition_point(|&r| r <= var);
        let (sum, count) = values[..cutoff]
            .iter()
            .fold((0.0, 0_usize), |(sum, count), &r| (sum + r, count + 1));
        Some(sum / count as f64)
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
    use std::collections::BTreeMap;

    use nautilus_core::{UnixNanos, approx_eq};
    use rstest::rstest;

    use super::*;
    use crate::statistics::value_at_risk::ValueAtRisk;

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
        let es = ExpectedShortfall::new(None);
        assert_eq!(es.name(), "Expected Shortfall (confidence 0.95)");
    }

    #[rstest]
    fn test_empty_returns() {
        let es = ExpectedShortfall::new(None);
        let returns = create_returns(&[]);
        let result = es.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_expected_shortfall_calculation() {
        // VaR(0.95) = -0.091 (see value_at_risk tests); the only return at or below
        // that threshold is -0.10, so ES = mean([-0.10]) = -0.10.
        let es = ExpectedShortfall::new(Some(0.95));
        let returns = create_returns(&[
            0.02, -0.05, 0.01, -0.08, 0.03, -0.02, 0.04, -0.10, 0.015, -0.03,
        ]);
        let result = es.calculate_from_returns(&returns).unwrap();
        assert!(approx_eq!(f64, result, -0.10, epsilon = 1e-12));
    }

    #[rstest]
    fn test_expected_shortfall_at_most_value_at_risk() {
        // ES is always <= VaR (it averages the tail beyond the threshold).
        let returns = create_returns(&[
            0.02, -0.05, 0.01, -0.08, 0.03, -0.02, 0.04, -0.10, 0.015, -0.03,
        ]);
        let var = ValueAtRisk::new(Some(0.90))
            .calculate_from_returns(&returns)
            .unwrap();
        let es = ExpectedShortfall::new(Some(0.90))
            .calculate_from_returns(&returns)
            .unwrap();
        assert!(es <= var);
    }

    #[rstest]
    fn test_expected_shortfall_averages_multi_element_tail() {
        // At confidence 0.60 the VaR threshold is -0.024, so four returns
        // (-0.10, -0.08, -0.05, -0.03) fall at or below it and ES averages all
        // four: mean = -0.26 / 4 = -0.065. Exercises the multi-element tail mean
        // (the other tests each produce a single-element tail).
        let es = ExpectedShortfall::new(Some(0.60));
        let returns = create_returns(&[
            0.02, -0.05, 0.01, -0.08, 0.03, -0.02, 0.04, -0.10, 0.015, -0.03,
        ]);
        let result = es.calculate_from_returns(&returns).unwrap();
        assert!(approx_eq!(f64, result, -0.065, epsilon = 1e-12));
    }

    #[rstest]
    #[case(Some(0.0))]
    #[case(Some(1.0))]
    #[case(Some(1.5))]
    #[case(Some(-0.5))]
    #[case(Some(f64::NAN))]
    #[case(Some(f64::INFINITY))]
    fn test_new_checked_rejects_invalid_confidence(#[case] confidence: Option<f64>) {
        assert!(ExpectedShortfall::new_checked(confidence).is_err());
    }

    #[rstest]
    #[case(None)]
    #[case(Some(0.5))]
    #[case(Some(0.99))]
    fn test_new_checked_accepts_valid_confidence(#[case] confidence: Option<f64>) {
        assert!(ExpectedShortfall::new_checked(confidence).is_ok());
    }
}
