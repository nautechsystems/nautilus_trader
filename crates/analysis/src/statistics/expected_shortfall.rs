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

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic, statistics::value_at_risk::ValueAtRisk};

/// Calculates the historical Expected Shortfall (Conditional Value at Risk) of
/// portfolio returns.
///
/// Expected Shortfall is the average of the losses that occur beyond the
/// [`ValueAtRisk`] threshold at a given confidence level — the mean of the worst
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
    /// Creates a new [`ExpectedShortfall`] instance.
    #[must_use]
    pub fn new(confidence: Option<f64>) -> Self {
        Self {
            confidence: confidence.unwrap_or(0.95),
        }
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

        // The VaR threshold at the same confidence (composes `ValueAtRisk`).
        let var = ValueAtRisk::new(Some(self.confidence)).calculate_from_returns(raw_returns)?;
        if var.is_nan() {
            return Some(f64::NAN);
        }

        let returns = self.downsample_to_daily_bins(raw_returns);
        let tail: Vec<f64> = returns.values().copied().filter(|&r| r <= var).collect();
        if tail.is_empty() {
            return Some(var);
        }

        Some(tail.iter().sum::<f64>() / tail.len() as f64)
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
}
