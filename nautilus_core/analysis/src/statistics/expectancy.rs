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

use super::{loser_avg::AvgLoser, winner_avg::AvgWinner};
use crate::statistic::PortfolioStatistic;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct Expectancy {}

impl PortfolioStatistic for Expectancy {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(Expectancy).to_string()
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        if realized_pnls.is_empty() {
            return Some(0.0);
        }

        let avg_winner = AvgWinner {}
            .calculate_from_realized_pnls(realized_pnls)
            .unwrap_or(0.0);
        let avg_loser = AvgLoser {}
            .calculate_from_realized_pnls(realized_pnls)
            .unwrap_or(0.0);

        let (winners, losers): (Vec<f64>, Vec<f64>) =
            realized_pnls.iter().partition(|&&pnl| pnl > 0.0);

        let total_trades = winners.len() + losers.len();
        let win_rate = winners.len() as f64 / total_trades.max(1) as f64;
        let loss_rate = 1.0 - win_rate;

        Some(avg_winner.mul_add(win_rate, avg_loser * loss_rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pnl_list() {
        let expectancy = Expectancy {};
        let result = expectancy.calculate_from_realized_pnls(&[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[test]
    fn test_all_winners() {
        let expectancy = Expectancy {};
        let pnls = vec![10.0, 20.0, 30.0];
        let result = expectancy.calculate_from_realized_pnls(&pnls);

        assert!(result.is_some());
        // Expected: avg_winner = 20.0, win_rate = 1.0, loss_rate = 0.0
        // Expectancy = (20.0 * 1.0) + (0.0 * 0.0) = 20.0
        assert_eq!(result.unwrap(), 20.0);
    }

    #[test]
    fn test_all_losers() {
        let expectancy = Expectancy {};
        let pnls = vec![-10.0, -20.0, -30.0];
        let result = expectancy.calculate_from_realized_pnls(&pnls);

        assert!(result.is_some());
        // Expected: avg_loser = -20.0, win_rate = 0.0, loss_rate = 1.0
        // Expectancy = (0.0 * 0.0) + (-20.0 * 1.0) = -20.0
        assert_eq!(result.unwrap(), -20.0);
    }

    #[test]
    fn test_mixed_pnls() {
        let expectancy = Expectancy {};
        let pnls = vec![10.0, -5.0, 15.0, -10.0];
        let result = expectancy.calculate_from_realized_pnls(&pnls);

        assert!(result.is_some());
        // Expected:
        // avg_winner = 12.5 (average of 10.0 and 15.0)
        // avg_loser = -7.5 (average of -5.0 and -10.0)
        // win_rate = 0.5 (2 winners out of 4 trades)
        // loss_rate = 0.5
        // Expectancy = (12.5 * 0.5) + (-7.5 * 0.5) = 2.5
        assert_eq!(result.unwrap(), 2.5);
    }

    #[test]
    fn test_single_trade() {
        let expectancy = Expectancy {};
        let pnls = vec![10.0];
        let result = expectancy.calculate_from_realized_pnls(&pnls);

        assert!(result.is_some());
        // Expected: avg_winner = 10.0, win_rate = 1.0, loss_rate = 0.0
        // Expectancy = (10.0 * 1.0) + (0.0 * 0.0) = 10.0
        assert_eq!(result.unwrap(), 10.0);
    }

    #[test]
    fn test_name() {
        let expectancy = Expectancy {};
        assert_eq!(expectancy.name(), "Expectancy");
    }
}
