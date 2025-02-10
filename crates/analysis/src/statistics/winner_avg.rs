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
pub struct AvgWinner {}

impl PortfolioStatistic for AvgWinner {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(AvgWinner).to_string()
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        if realized_pnls.is_empty() {
            return Some(0.0);
        }

        let winners: Vec<f64> = realized_pnls
            .iter()
            .filter(|&&pnl| pnl > 0.0)
            .copied()
            .collect();

        if winners.is_empty() {
            return Some(0.0);
        }

        let sum: f64 = winners.iter().sum();
        Some(sum / winners.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pnls() {
        let avg_winner = AvgWinner {};
        let result = avg_winner.calculate_from_realized_pnls(&[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[test]
    fn test_no_winning_trades() {
        let avg_winner = AvgWinner {};
        let realized_pnls = vec![-100.0, -50.0, -200.0];
        let result = avg_winner.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[test]
    fn test_all_winning_trades() {
        let avg_winner = AvgWinner {};
        let realized_pnls = vec![100.0, 50.0, 200.0];
        let result = avg_winner.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 116.66666666666667);
    }

    #[test]
    fn test_mixed_trades() {
        let avg_winner = AvgWinner {};
        let realized_pnls = vec![100.0, -50.0, 200.0, -100.0];
        let result = avg_winner.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 150.0);
    }

    #[test]
    fn test_name() {
        let avg_winner = AvgWinner {};
        assert_eq!(avg_winner.name(), "AvgWinner");
    }
}
