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
pub struct RiskReturnRatio {}

impl PortfolioStatistic for RiskReturnRatio {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(RiskReturnRatio).to_string()
    }

    fn calculate_from_returns(&self, returns: &Returns) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        let mean = returns.values().sum::<f64>() / returns.len() as f64;
        let std = self.calculate_std(returns);

        if std < f64::EPSILON {
            Some(f64::NAN)
        } else {
            Some(mean / std)
        }
    }
}

#[cfg(test)]
mod tests {
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
        let ratio = RiskReturnRatio {};
        let returns = create_returns(vec![]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_zero_std_dev() {
        let ratio = RiskReturnRatio {};
        let returns = create_returns(vec![0.05; 10]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert!(result.unwrap().is_nan());
    }

    #[rstest]
    fn test_valid_risk_return_ratio() {
        let ratio = RiskReturnRatio {};
        let returns = create_returns(vec![0.1, -0.05, 0.2, -0.1, 0.15]);
        let result = ratio.calculate_from_returns(&returns);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.46360044557175345);
    }

    #[rstest]
    fn test_name() {
        let ratio = RiskReturnRatio {};
        assert_eq!(ratio.name(), "RiskReturnRatio");
    }
}
