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
pub struct ProfitFactor {}

impl PortfolioStatistic for ProfitFactor {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(ProfitFactor).to_string()
    }

    fn calculate_from_returns(&mut self, returns: &[f64]) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        let (positive_returns_sum, negative_returns_sum): (f64, f64) =
            returns.iter().fold((0.0, 0.0), |(pos_sum, neg_sum), &pnl| {
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
