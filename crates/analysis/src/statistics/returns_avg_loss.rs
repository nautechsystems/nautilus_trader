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

#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct ReturnsAverageLoss {}

impl Display for ReturnsAverageLoss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Average Loss (Return)")
    }
}

impl PortfolioStatistic for ReturnsAverageLoss {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_returns(&self, returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        let negative_returns: Vec<f64> = returns.values().copied().filter(|&x| x < 0.0).collect();

        if negative_returns.is_empty() {
            return Some(f64::NAN);
        }

        let sum: f64 = negative_returns.iter().sum();
        let count = negative_returns.len() as f64;

        Some(sum / count)
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
        for (i, value) in values.iter().enumerate() {
            new_return.insert(UnixNanos::from(i as u64), *value);
        }

        new_return
    }

    #[rstest]
    fn test_empty_returns() {
        let avg_loss = ReturnsAverageLoss {};
        let returns = create_returns(vec![]);
        let result = avg_loss.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_all_positive() {
        let avg_loss = ReturnsAverageLoss {};
        let returns = create_returns(vec![10.0, 20.0, 30.0]);
        let result = avg_loss.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_all_negative() {
        let avg_loss = ReturnsAverageLoss {};
        let returns = create_returns(vec![-10.0, -20.0, -30.0]);
        let result = avg_loss.calculate_from_returns(&returns);
        assert!(result.is_some());
        // Average of [-10.0, -20.0, -30.0] = (-10 + -20 + -30) / 3 = -20.0
        assert!(approx_eq!(f64, result.unwrap(), -20.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_mixed_returns() {
        let avg_loss = ReturnsAverageLoss {};
        let returns = create_returns(vec![10.0, -20.0, 30.0, -40.0]);
        let result = avg_loss.calculate_from_returns(&returns);
        assert!(result.is_some());
        // Average of [-20.0, -40.0] = (-20 + -40) / 2 = -30.0
        assert!(approx_eq!(f64, result.unwrap(), -30.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_with_zero() {
        let avg_loss = ReturnsAverageLoss {};
        let returns = create_returns(vec![10.0, 0.0, -20.0, -30.0]);
        let result = avg_loss.calculate_from_returns(&returns);
        assert!(result.is_some());
        // Average of [-20.0, -30.0] = (-20 + -30) / 2 = -25.0
        assert!(approx_eq!(f64, result.unwrap(), -25.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_name() {
        let avg_loss = ReturnsAverageLoss {};
        assert_eq!(avg_loss.name(), "Average Loss (Return)");
    }
}
