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
pub struct WinRate {}

impl PortfolioStatistic for WinRate {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(WinRate).to_string()
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        if realized_pnls.is_empty() {
            return Some(0.0);
        }

        let (winners, losers): (Vec<f64>, Vec<f64>) =
            realized_pnls.iter().partition(|&&pnl| pnl > 0.0);

        let total_trades = winners.len() + losers.len();
        Some(winners.len() as f64 / total_trades.max(1) as f64)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_empty_pnls() {
        let win_rate = WinRate {};
        let result = win_rate.calculate_from_realized_pnls(&[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[rstest]
    fn test_all_winning_trades() {
        let win_rate = WinRate {};
        let realized_pnls = vec![100.0, 50.0, 200.0];
        let result = win_rate.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 1.0);
    }

    #[rstest]
    fn test_all_losing_trades() {
        let win_rate = WinRate {};
        let realized_pnls = vec![-100.0, -50.0, -200.0];
        let result = win_rate.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.0);
    }

    #[rstest]
    fn test_mixed_trades() {
        let win_rate = WinRate {};
        let realized_pnls = vec![100.0, -50.0, 200.0, -100.0];
        let result = win_rate.calculate_from_realized_pnls(&realized_pnls);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0.5);
    }

    #[rstest]
    fn test_name() {
        let win_rate = WinRate {};
        assert_eq!(win_rate.name(), "WinRate");
    }
}
