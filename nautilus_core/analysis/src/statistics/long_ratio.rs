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

use nautilus_model::{enums::OrderSide, position::Position};

use crate::statistic::PortfolioStatistic;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]

pub struct LongRatio {
    precision: usize,
}

impl LongRatio {
    #[must_use]
    pub fn new(precision: Option<usize>) -> Self {
        Self {
            precision: precision.unwrap_or(2),
        }
    }
}

impl PortfolioStatistic for LongRatio {
    type Item = String;

    fn name(&self) -> String {
        stringify!(LongRatio).to_string()
    }

    fn calculate_from_positions(&mut self, positions: &[Position]) -> Option<Self::Item> {
        if positions.is_empty() {
            return None;
        }

        let longs: Vec<&Position> = positions
            .iter()
            .filter(|p| matches!(p.entry, OrderSide::Buy))
            .collect();

        let value = longs.len() as f64 / positions.len() as f64;
        Some(format!("{:.1$}", value, self.precision))
    }
}
