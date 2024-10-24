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
pub struct RiskReturnRatio {}

impl PortfolioStatistic for RiskReturnRatio {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(RiskReturnRatio).to_string()
    }

    fn calculate_from_returns(&mut self, returns: &[f64]) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        // let negative_returns: &[f64] = returns.iter().copied().filter(|&x| x != 0.0).collect();

        // if negative_returns.is_empty() {
        //     return Some(f64::NAN);
        // }

        // let sum: f64 = negative_returns.iter().sum();
        // let downsampled_returns = self.downsample_to_daily_bins(returns);
        // let count = negative_returns.len() as f64;

        // Some(sum / count)
        todo!()
    }
}
