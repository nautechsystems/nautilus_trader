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
pub struct ReturnsAverage {}

impl PortfolioStatistic for ReturnsAverage {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(ReturnsAverage).to_string()
    }

    fn calculate_from_returns(&self, returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        let negative_returns: Vec<f64> = returns.values().copied().filter(|&x| x != 0.0).collect();

        if negative_returns.is_empty() {
            return Some(f64::NAN);
        }

        let sum: f64 = negative_returns.iter().sum();
        let count = negative_returns.len() as f64;

        Some(sum / count)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nautilus_core::UnixNanos;

    use super::*;

    fn create_returns(values: Vec<f64>) -> Returns {
        let mut new_return = BTreeMap::new();
        for (i, value) in values.iter().enumerate() {
            new_return.insert(UnixNanos::from(i as u64), *value);
        }
        new_return
    }

    #[test]
    fn test_empty_returns() {
        let avg = ReturnsAverage {};
        let returns = create_returns(vec![]);
        let result = avg.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[test]
    fn test_all_zero() {
        let avg = ReturnsAverage {};
        let returns = create_returns(vec![0.0, 0.0, 0.0]);
        let result = avg.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[test]
    fn test_mixed_non_zero() {
        let avg = ReturnsAverage {};
        let returns = create_returns(vec![10.0, -20.0, 0.0, 30.0, -40.0]);
        let result = avg.calculate_from_returns(&returns);
        assert!(result.is_some());
        // Average of [10.0, -20.0, 30.0, -40.0] = (-20 + -40 + 10 + 30) / 4 = -5.0
        assert_eq!(result.unwrap(), -5.0);
    }

    #[test]
    fn test_name() {
        let avg = ReturnsAverage {};
        assert_eq!(avg.name(), "ReturnsAverage");
    }
}
