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

use super::{loser_avg::AvgLoser, winner_avg::AvgWinner};
use crate::{Returns, statistic::PortfolioStatistic};

/// Calculates the expectancy of a trading strategy based on realized PnLs.
///
/// Expectancy is defined as: `(Average Win × Win Rate) + (Average Loss × Loss Rate)`
/// This metric provides insight into the expected profitability per trade and helps
/// evaluate the overall edge of a trading strategy.
///
/// A positive expectancy indicates a profitable system over time, while a negative
/// expectancy suggests losses.
///
/// # References
///
/// - Tharp, V. K. (1998). *Trade Your Way to Financial Freedom*. McGraw-Hill.
/// - Elder, A. (1993). *Trading for a Living*. John Wiley & Sons.
/// - Vince, R. (1992). *The Mathematics of Money Management*. John Wiley & Sons.
#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct Expectancy {}

impl Display for Expectancy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Expectancy")
    }
}

impl PortfolioStatistic for Expectancy {
    type Item = f64;

    fn name(&self) -> String {
        self.to_string()
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
    fn calculate_from_returns(&self, _returns: &Returns) -> Option<Self::Item> {
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
    use nautilus_core::approx_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_empty_pnl_list() {
        let expectancy = Expectancy {};
        let result = expectancy.calculate_from_realized_pnls(&[]);
        assert!(result.is_some());
        assert!(approx_eq!(f64, result.unwrap(), 0.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_all_winners() {
        let expectancy = Expectancy {};
        let pnls = vec![10.0, 20.0, 30.0];
        let result = expectancy.calculate_from_realized_pnls(&pnls);

        assert!(result.is_some());
        // Expected: avg_winner = 20.0, win_rate = 1.0, loss_rate = 0.0
        // Expectancy = (20.0 * 1.0) + (0.0 * 0.0) = 20.0
        assert!(approx_eq!(f64, result.unwrap(), 20.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_all_losers() {
        let expectancy = Expectancy {};
        let pnls = vec![-10.0, -20.0, -30.0];
        let result = expectancy.calculate_from_realized_pnls(&pnls);

        assert!(result.is_some());
        // Expected: avg_loser = -20.0, win_rate = 0.0, loss_rate = 1.0
        // Expectancy = (0.0 * 0.0) + (-20.0 * 1.0) = -20.0
        assert!(approx_eq!(f64, result.unwrap(), -20.0, epsilon = 1e-9));
    }

    #[rstest]
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
        assert!(approx_eq!(f64, result.unwrap(), 2.5, epsilon = 1e-9));
    }

    #[rstest]
    fn test_single_trade() {
        let expectancy = Expectancy {};
        let pnls = vec![10.0];
        let result = expectancy.calculate_from_realized_pnls(&pnls);

        assert!(result.is_some());
        // Expected: avg_winner = 10.0, win_rate = 1.0, loss_rate = 0.0
        // Expectancy = (10.0 * 1.0) + (0.0 * 0.0) = 10.0
        assert!(approx_eq!(f64, result.unwrap(), 10.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_name() {
        let expectancy = Expectancy {};
        assert_eq!(expectancy.name(), "Expectancy");
    }
}
