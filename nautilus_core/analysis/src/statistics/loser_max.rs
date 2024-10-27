// # -------------------------------------------------------------------------------------------------
// #  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
// #  https://nautechsystems.io
// #
// #  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
// #  You may not use this file except in compliance with the License.
// #  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// #
// #  Unless required by applicable law or agreed to in writing, software
// #  distributed under the License is distributed on an "AS IS" BASIS,
// #  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// #  See the License for the specific language governing permissions and
// #  limitations under the License.
// # -------------------------------------------------------------------------------------------------

use crate::statistic::PortfolioStatistic;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct MaxLoser {}

impl PortfolioStatistic for MaxLoser {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(MaxLoser).to_string()
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        if realized_pnls.is_empty() {
            return Some(0.0);
        }

        realized_pnls
            .iter()
            .copied()
            .filter(|&pnl| pnl < 0.0)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pnls() {
        let max_loser = MaxLoser {};
        let result = max_loser.calculate_from_realized_pnls(&[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[test]
    fn test_all_positive() {
        let max_loser = MaxLoser {};
        let pnls = vec![10.0, 20.0, 30.0];
        let result = max_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_none());
    }

    #[test]
    fn test_all_negative() {
        let max_loser = MaxLoser {};
        let pnls = vec![-10.0, -20.0, -30.0];
        let result = max_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), -30.0);
    }

    #[test]
    fn test_mixed_pnls() {
        let max_loser = MaxLoser {};
        let pnls = vec![10.0, -20.0, 30.0, -40.0];
        let result = max_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), -40.0);
    }

    #[test]
    fn test_with_zero() {
        let max_loser = MaxLoser {};
        let pnls = vec![10.0, 0.0, -20.0, -30.0];
        let result = max_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), -30.0);
    }

    #[test]
    fn test_single_value() {
        let max_loser = MaxLoser {};
        let pnls = vec![-10.0];
        let result = max_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), -10.0);
    }

    #[test]
    fn test_name() {
        let max_loser = MaxLoser {};
        assert_eq!(max_loser.name(), "MaxLoser");
    }
}
