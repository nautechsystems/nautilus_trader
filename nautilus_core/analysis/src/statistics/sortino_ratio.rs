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
pub struct SortinoRatio {
    period: usize,
}

impl SortinoRatio {
    #[must_use]
    pub fn new(period: Option<usize>) -> Self {
        Self {
            period: period.unwrap_or(252),
        }
    }
}

impl PortfolioStatistic for SortinoRatio {
    type Item = f64;

    fn name(&self) -> String {
        stringify!(SortinoRatio).to_string()
    }

    fn calculate_from_returns(&mut self, returns: &[f64]) -> Option<Self::Item> {
        if !self.check_valid_returns(returns) {
            return Some(f64::NAN);
        }

        let total_n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / total_n;

        let downside = (returns
            .iter()
            .filter(|&&x| x < 0.0)
            .map(|x| x.powi(2))
            .sum::<f64>()
            / total_n)
            .sqrt();

        if downside < f64::EPSILON {
            return Some(f64::NAN);
        }

        let annualized_ratio = (mean / downside) * (self.period as f64).sqrt();

        Some(annualized_ratio)
    }
}
