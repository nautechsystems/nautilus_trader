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
pub struct ProfitFactor {}

impl PortfolioStatistic for ProfitFactor {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(ProfitFactor).to_string()
    }

    fn calculate_from_returns(&self, returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        let (positive_returns_sum, negative_returns_sum) =
            returns
                .values()
                .fold((0.0, 0.0), |(pos_sum, neg_sum), &pnl| {
                    if pnl >= 0.0 {
                        (pos_sum + pnl, neg_sum)
                    } else {
                        (pos_sum, neg_sum + pnl)
                    }
                });

        if negative_returns_sum == 0.0 {
            return Some(f64::NAN);
        }
        Some((positive_returns_sum / negative_returns_sum).abs())
    }
}

#[cfg(test)]
mod profit_factor_tests {
    use std::collections::BTreeMap;

    use nautilus_core::UnixNanos;
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
        let profit_factor = ProfitFactor {};
        let returns = create_returns(vec![]);
        let result = profit_factor.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_all_positive() {
        let profit_factor = ProfitFactor {};
        let returns = create_returns(vec![10.0, 20.0, 30.0]);
        let result = profit_factor.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_all_negative() {
        let profit_factor = ProfitFactor {};
        let returns = create_returns(vec![-10.0, -20.0, -30.0]);
        let result = profit_factor.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[rstest]
    fn test_mixed_returns() {
        let profit_factor = ProfitFactor {};
        let returns = create_returns(vec![10.0, -20.0, 30.0, -40.0]);
        let result = profit_factor.calculate_from_returns(&returns);
        assert!(result.is_some());
        // (10.0 + 30.0) / |-20.0 + -40.0| = 40 / 60 = 0.666...
        assert_eq!(result.unwrap(), 0.6666666666666666);
    }

    #[rstest]
    fn test_with_zero() {
        let profit_factor = ProfitFactor {};
        let returns = create_returns(vec![10.0, 0.0, -20.0, -30.0]);
        let result = profit_factor.calculate_from_returns(&returns);
        assert!(result.is_some());
        // (10.0 + 0.0) / |-20.0 + -30.0| = 10 / 50 = 0.2
        assert_eq!(result.unwrap(), 0.2);
    }

    #[rstest]
    fn test_equal_positive_negative() {
        let profit_factor = ProfitFactor {};
        let returns = create_returns(vec![20.0, -20.0]);
        let result = profit_factor.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 1.0);
    }

    #[rstest]
    fn test_name() {
        let profit_factor = ProfitFactor {};
        assert_eq!(profit_factor.name(), "ProfitFactor");
    }
}
