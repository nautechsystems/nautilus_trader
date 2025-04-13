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

use crate::statistic::PortfolioStatistic;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct AvgLoser {}

impl PortfolioStatistic for AvgLoser {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(AvgLoser).to_string()
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        if realized_pnls.is_empty() {
            return Some(0.0);
        }

        let losers: Vec<f64> = realized_pnls
            .iter()
            .filter(|&&pnl| pnl <= 0.0)
            .copied()
            .collect();

        if losers.is_empty() {
            return Some(0.0);
        }

        let sum: f64 = losers.iter().sum();
        Some(sum / losers.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_empty_pnls() {
        let avg_loser = AvgLoser {};
        let result = avg_loser.calculate_from_realized_pnls(&[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[rstest]
    fn test_no_losers() {
        let avg_loser = AvgLoser {};
        let pnls = vec![10.0, 20.0, 30.0];
        let result = avg_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[rstest]
    fn test_only_losers() {
        let avg_loser = AvgLoser {};
        let pnls = vec![-10.0, -20.0, -30.0];
        let result = avg_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), -20.0);
    }

    #[rstest]
    fn test_mixed_pnls() {
        let avg_loser = AvgLoser {};
        let pnls = vec![10.0, -20.0, 30.0, -40.0];
        let result = avg_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), -30.0);
    }

    #[rstest]
    fn test_zero_included() {
        let avg_loser = AvgLoser {};
        let pnls = vec![10.0, 0.0, -20.0, -30.0];
        let result = avg_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        // Average of [0.0, -20.0, -30.0]
        assert_eq!(result.unwrap(), -16.666666666666668);
    }

    #[rstest]
    fn test_single_loser() {
        let avg_loser = AvgLoser {};
        let pnls = vec![-10.0];
        let result = avg_loser.calculate_from_realized_pnls(&pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), -10.0);
    }

    #[rstest]
    fn test_name() {
        let avg_loser = AvgLoser {};
        assert_eq!(avg_loser.name(), "AvgLoser");
    }
}
