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

use super::{loser_avg::AvgLoser, winner_avg::AvgWinner};
use crate::statistic::PortfolioStatistic;

/// An indicator which calculates an adaptive moving average (AMA) across a
/// rolling window. Developed by Perry Kaufman, the AMA is a moving average
/// designed to account for market noise and volatility. The AMA will closely
/// follow prices when the price swings are relatively small and the noise is
/// low. The AMA will increase lag when the price swings increase.
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
