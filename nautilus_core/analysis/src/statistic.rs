// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::{orders::base::Order, position::Position};

const IMPL_ERR: &str = "is not implemented for";

#[allow(unused_variables)]
pub trait PortfolioStatistic {
    type Item;

    fn name(&self) -> String;

    #[allow(dead_code)]
    fn calculate_from_returns(&mut self, returns: &[f64]) -> Option<Self::Item> {
        panic!("`calculate_from_returns` {IMPL_ERR} `{}`", self.name());
    }

    fn calculate_from_realized_pnls(&mut self, realized_pnls: &[f64]) -> Option<Self::Item> {
        panic!(
            "`calculate_from_realized_pnls` {IMPL_ERR} `{}`",
            self.name()
        );
    }

    #[allow(dead_code)]
    fn calculate_from_orders(&mut self, orders: Vec<impl Order>) -> Option<Self::Item> {
        panic!("`calculate_from_orders` {IMPL_ERR} `{}`", self.name());
    }

    #[allow(dead_code)]
    fn calculate_from_positions(&mut self, positions: &[Position]) -> Option<Self::Item> {
        panic!("`calculate_from_positions` {IMPL_ERR} `{}`", self.name());
    }

    #[allow(dead_code)]
    fn check_valid_returns(&self, returns: &[f64]) -> bool {
        !returns.is_empty()
    }

    #[allow(dead_code)]
    fn downsample_to_daily_bins(&self, returns: &[f64]) -> &Vec<f64> {
        // return returns.dropna().resample("1D").sum()
        panic!("`downsample_to_daily_bins` {IMPL_ERR} `{}`", self.name());
    }
}
