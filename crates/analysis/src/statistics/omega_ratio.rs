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

//! Omega Ratio statistic.

use std::fmt::Display;

use nautilus_model::position::Position;

use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the Omega ratio of portfolio returns.
///
/// The Omega ratio is the ratio of probability-weighted gains to losses relative
/// to a return threshold `θ`. It captures the entire return distribution (all
/// moments), unlike the Sharpe ratio which only uses the first two:
///
/// `Omega(θ) = sum(max(r - θ, 0)) / sum(max(θ - r, 0))`
///
/// The threshold `θ` defaults to `0` (gains vs losses about zero). A value above
/// `1` means gains above the threshold outweigh losses below it. Returns `NaN`
/// for an empty series, or when there are no returns below the threshold (the
/// ratio is undefined).
///
/// # References
///
/// - Keating, C., & Shadwick, W. F. (2002). "A Universal Performance Measure".
///   *Journal of Performance Measurement*, 6(3), 59-84.
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
pub struct OmegaRatio {
    /// The return threshold `θ` separating gains from losses (default: 0.0).
    threshold: f64,
}

impl OmegaRatio {
    /// Creates a new [`OmegaRatio`] instance.
    #[must_use]
    pub fn new(threshold: Option<f64>) -> Self {
        Self {
            threshold: threshold.unwrap_or(0.0),
        }
    }
}

impl Display for OmegaRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Omega Ratio (threshold {})", self.threshold)
    }
}

impl PortfolioStatistic for OmegaRatio {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_returns(&self, raw_returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(raw_returns) {
            return Some(f64::NAN);
        }

        let returns = self.downsample_to_daily_bins(raw_returns);

        let mut gain = 0.0;
        let mut loss = 0.0;

        for &ret in returns.values() {
            let excess = ret - self.threshold;
            if excess > 0.0 {
                gain += excess;
            } else {
                loss -= excess;
            }
        }

        if loss <= 0.0 {
            return Some(f64::NAN);
        }

        Some(gain / loss)
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
        let ratio = OmegaRatio::new(None);
        assert_eq!(ratio.name(), "Omega Ratio (threshold 0)");
    }

    #[rstest]
    fn test_empty_returns() {
        let ratio = OmegaRatio::new(None);
        let returns = create_returns(&[]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_no_losses_is_nan() {
        // No returns below the threshold leaves the ratio undefined.
        let ratio = OmegaRatio::new(None);
        let returns = create_returns(&[0.01, 0.02, 0.015]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_omega_ratio_calculation() {
        // Gains above 0: 0.01 + 0.015 + 0.025 = 0.05.
        // Losses below 0: 0.02 + 0.005 = 0.025.
        // Omega(0) = 0.05 / 0.025 = 2.0.
        let ratio = OmegaRatio::new(Some(0.0));
        let returns = create_returns(&[0.01, -0.02, 0.015, -0.005, 0.025]);
        let result = ratio.calculate_from_returns(&returns).unwrap();
        assert!(approx_eq!(f64, result, 2.0, epsilon = 1e-12));
    }
}
