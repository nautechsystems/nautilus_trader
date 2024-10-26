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

use std::fmt::Debug;

use nautilus_model::{orders::base::Order, position::Position};

use crate::Returns;

const IMPL_ERR: &str = "is not implemented for";

#[allow(unused_variables)]
pub trait PortfolioStatistic: Debug {
    type Item;

    fn name(&self) -> String;

    fn calculate_from_returns(&self, returns: &Returns) -> Option<Self::Item> {
        panic!("`calculate_from_returns` {IMPL_ERR} `{}`", self.name());
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<Self::Item> {
        panic!(
            "`calculate_from_realized_pnls` {IMPL_ERR} `{}`",
            self.name()
        );
    }

    #[allow(dead_code)]
    fn calculate_from_orders(&self, orders: Vec<Box<dyn Order>>) -> Option<Self::Item> {
        panic!("`calculate_from_orders` {IMPL_ERR} `{}`", self.name());
    }

    #[allow(dead_code)]
    fn calculate_from_positions(&self, positions: &[Position]) -> Option<Self::Item> {
        panic!("`calculate_from_positions` {IMPL_ERR} `{}`", self.name());
    }

    fn check_valid_returns(&self, returns: &Returns) -> bool {
        !returns.is_empty()
    }

    // TODO: Future enhancement - implement proper downsampling using the Polars library.
    // Currently, we have only a 1D array of returns, so we can’t perform time-based resampling as we could with a DataFrame in Python (e.g., Pandas).
    // In Python, the data structure supports easy time-based resampling, but here, we’ll need to use Polars' Series with a DateTime index
    // to enable similar resampling capabilities in Rust.
    // Example future function signature:
    // fn downsample_to_daily_bins(&self, returns: &polars::Series) -> polars::Series
    fn downsample_to_daily_bins(&self, returns: &[f64]) -> Vec<f64> {
        // For now, we return the input array directly, assuming daily data
        // Future implementation will include time-based resampling, e.g., returns.dropna().resample("1D").sum()
        returns.to_vec()
    }

    fn calculate_std(&self, returns: &Returns) -> f64 {
        let n = returns.len() as f64;
        if n < 2.0 {
            return f64::NAN;
        }

        let mean = returns.values().sum::<f64>() / n;

        let variance = returns.values().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);

        variance.sqrt()
    }
}
