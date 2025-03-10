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
pub struct MinWinner {}

impl PortfolioStatistic for MinWinner {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(MinWinner).to_string()
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        if realized_pnls.is_empty() {
            return Some(0.0);
        }

        realized_pnls
            .iter()
            .filter(|&&pnl| pnl > 0.0)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pnls() {
        let min_winner = MinWinner {};
        let result = min_winner.calculate_from_realized_pnls(&[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[test]
    fn test_no_winning_trades() {
        let min_winner = MinWinner {};
        let realized_pnls = vec![-100.0, -50.0, -200.0];
        let result = min_winner.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_none());
    }

    #[test]
    fn test_all_winning_trades() {
        let min_winner = MinWinner {};
        let realized_pnls = vec![100.0, 50.0, 200.0];
        let result = min_winner.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 50.0); // Minimum of 100.0, 50.0, and 200.0 is 50.0
    }

    #[test]
    fn test_mixed_trades() {
        let min_winner = MinWinner {};
        let realized_pnls = vec![100.0, -50.0, 200.0, -100.0];
        let result = min_winner.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 100.0); // Minimum of 100.0 and 200.0 is 100.0
    }

    #[test]
    fn test_single_winning_trade() {
        let min_winner = MinWinner {};
        let realized_pnls = vec![50.0];
        let result = min_winner.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 50.0);
    }

    #[test]
    fn test_name() {
        let min_winner = MinWinner {};
        assert_eq!(min_winner.name(), "MinWinner");
    }
}
