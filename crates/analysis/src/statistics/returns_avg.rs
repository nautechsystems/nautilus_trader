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

#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct ReturnsAverage {}

impl Display for ReturnsAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Average (Return)")
    }
}

impl PortfolioStatistic for ReturnsAverage {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
    }

    fn calculate_from_returns(&self, returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        let sum: f64 = returns.values().sum();
        let count = returns.len() as f64;

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
        let avg = ReturnsAverage {};
        let returns = create_returns(vec![]);
        let result = avg.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_all_zero() {
        let avg = ReturnsAverage {};
        let returns = create_returns(vec![0.0, 0.0, 0.0]);
        let result = avg.calculate_from_returns(&returns);
        assert!(result.is_some());
        // Average of [0.0, 0.0, 0.0] = 0.0
        assert!(approx_eq!(f64, result.unwrap(), 0.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_mixed_with_zeros() {
        let avg = ReturnsAverage {};
        let returns = create_returns(vec![10.0, -20.0, 0.0, 30.0, -40.0]);
        let result = avg.calculate_from_returns(&returns);
        assert!(result.is_some());
        // Average of [10.0, -20.0, 0.0, 30.0, -40.0] = -20 / 5 = -4.0
        assert!(approx_eq!(f64, result.unwrap(), -4.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_zeros_included_in_average() {
        let avg = ReturnsAverage {};
        let returns = create_returns(vec![1.0, 0.0, 0.0]);
        let result = avg.calculate_from_returns(&returns);
        assert!(result.is_some());
        // Average of [1.0, 0.0, 0.0] = 1.0 / 3 = 0.333...
        assert!(approx_eq!(
            f64,
            result.unwrap(),
            0.3333333333333333,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_name() {
        let avg = ReturnsAverage {};
        assert_eq!(avg.name(), "Average (Return)");
    }
}
