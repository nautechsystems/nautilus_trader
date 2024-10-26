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

use std::collections::{BTreeMap, HashMap};

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{identifiers::PositionId, position::Position, types::currency::Currency};
use pyo3::prelude::*;

use crate::analyzer::PortfolioAnalyzer;

#[pymethods]
impl PortfolioAnalyzer {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    // fn py_register_statistic(
    //     &mut self,
    //     statistic: Statistic,
    // ) {
    //     self.register_statistic(statistic);
    // }

    // fn py_deregister_statistic(&mut self, statistic: Statistic) {
    //     self.deregister_statistic(statistic);
    // }

    fn py_deregister_statistics(&mut self) {
        self.deregister_statistics();
    }

    fn py_reset(&mut self) {
        self.reset();
    }

    fn py_currencies(&self) -> Vec<Currency> {
        self.currencies().into_iter().copied().collect()
    }

    // fn py_statistic(
    //     &self,
    //     name: &str,
    // ) -> Option<&Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync>> {
    //     self.statistic(name)
    // }

    fn py_returns(&self) -> BTreeMap<u64, f64> {
        self.returns()
            .iter()
            .map(|(k, v)| (k.clone().as_u64(), *v))
            .collect()
    }

    // fn py_calculate_statistics(&mut self, account: dyn Account, positions: [Position]) {
    //     self.calculate_statistics(&account, positions);
    // }

    fn py_add_positions(&mut self, positions: Vec<Position>) {
        self.add_positions(&positions);
    }

    // fn py_add_trade(&mut self, position_id: &PositionId, pnl: &Money) {
    //     self.add_trade(position_id, pnl);
    // }

    fn py_add_return(&mut self, timestamp: u64, value: f64) {
        self.add_return(UnixNanos::from(timestamp), value);
    }

    fn py_realized_pnls(&self, currency: Option<Currency>) -> Option<Vec<(PositionId, f64)>> {
        self.realized_pnls(currency.as_ref())
    }

    // fn py_total_pnl(
    //     &self,
    //     currency: Option<&Currency>,
    //     unrealized_pnl: Option<&Money>,
    // ) -> Result<f64, &'static str> {
    //     self.total_pnl(currency, unrealized_pnl)
    // }

    // fn py_total_pnl_percentage(
    //     &self,
    //     currency: Option<&Currency>,
    //     unrealized_pnl: Option<&Money>,
    // ) -> Result<f64, &'static str> {
    //     self.total_pnl_percentage(currency, unrealized_pnl)
    // }

    // fn py_get_performance_stats_pnls(
    //     &self,
    //     currency: Option<&Currency>,
    //     unrealized_pnl: Option<&Money>,
    // ) -> Result<HashMap<String, f64>, &'static str> {
    //     self.get_performance_stats_pnls(currency, unrealized_pnl)
    // }

    fn py_get_performance_stats_returns(&self) -> HashMap<String, f64> {
        self.get_performance_stats_returns()
    }

    fn py_get_performance_stats_general(&self) -> HashMap<String, f64> {
        self.get_performance_stats_general()
    }

    // fn py_get_stats_pnls_formatted(
    //     &self,
    //     currency: Option<&Currency>,
    //     unrealized_pnl: Option<&Money>,
    // ) -> Result<Vec<String>, String> {
    //     self.get_stats_pnls_formatted(currency, unrealized_pnl)
    // }

    fn py_get_stats_returns_formatted(&self) -> Vec<String> {
        self.get_stats_returns_formatted()
    }

    fn py_get_stats_general_formatted(&self) -> Vec<String> {
        self.get_stats_general_formatted()
    }
}
