// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use nautilus_model::{
    identifiers::PositionId,
    position::Position,
    types::{Currency, Money},
};
use pyo3::prelude::*;

use crate::{Returns, analyzer::PortfolioAnalyzer, python::statistic::statistic_from_pyobject};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PortfolioAnalyzer {
    /// Analyzes portfolio performance and calculates various statistics.
    ///
    /// The `PortfolioAnalyzer` tracks account balances, positions, and realized PnLs
    /// to provide portfolio analysis including returns, PnL calculations,
    /// and customizable statistics.
    #[new]
    #[must_use]
    pub fn py_new() -> Self {
        Self::new()
    }

    fn __repr__(&self) -> String {
        format!("PortfolioAnalyzer(currencies={})", self.currencies().len())
    }

    /// Returns all tracked currencies.
    #[pyo3(name = "currencies")]
    fn py_currencies(&self) -> Vec<Currency> {
        self.currencies().into_iter().copied().collect()
    }

    /// Gets all return-based performance statistics.
    #[pyo3(name = "get_performance_stats_returns")]
    fn py_get_performance_stats_returns(&self) -> HashMap<String, f64> {
        self.get_performance_stats_returns().into_iter().collect()
    }

    /// Gets all position-return-based performance statistics.
    #[pyo3(name = "get_performance_stats_position_returns")]
    fn py_get_performance_stats_position_returns(&self) -> HashMap<String, f64> {
        self.get_performance_stats_position_returns()
            .into_iter()
            .collect()
    }

    /// Gets all portfolio-return-based performance statistics.
    #[pyo3(name = "get_performance_stats_portfolio_returns")]
    fn py_get_performance_stats_portfolio_returns(&self) -> HashMap<String, f64> {
        self.get_performance_stats_portfolio_returns()
            .into_iter()
            .collect()
    }

    /// Gets all benchmark-relative return statistics for the primary returns.
    ///
    /// This is stateless: the `benchmark` series is supplied by the caller rather
    /// than stored on the analyzer. Only statistics that override
    /// `PortfolioStatistic.calculate_from_returns_with_benchmark` (the benchmark-relative
    /// statistics) contribute values; all others return `None` and are skipped.
    #[pyo3(name = "get_performance_stats_returns_vs_benchmark")]
    fn py_get_performance_stats_returns_vs_benchmark(
        &self,
        benchmark: BTreeMap<u64, f64>,
    ) -> HashMap<String, f64> {
        let benchmark: Returns = benchmark
            .into_iter()
            .map(|(k, v)| (UnixNanos::from(k), v))
            .collect();
        self.get_performance_stats_returns_vs_benchmark(&benchmark)
            .into_iter()
            .collect()
    }

    /// Gets all PnL-related performance statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if PnL calculations fail, for example due to:
    ///
    /// - No currency specified for a multi-currency portfolio.
    /// - Unrealized PnL currency not matching the specified currency.
    /// - Specified currency not found in account balances.
    #[pyo3(name = "get_performance_stats_pnls")]
    fn py_get_performance_stats_pnls(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> PyResult<HashMap<String, f64>> {
        self.get_performance_stats_pnls(currency, unrealized_pnl)
            .map(|m| m.into_iter().collect())
            .map_err(to_pyvalue_err)
    }

    /// Gets general portfolio statistics.
    #[pyo3(name = "get_performance_stats_general")]
    fn py_get_performance_stats_general(&self) -> HashMap<String, f64> {
        self.get_performance_stats_general().into_iter().collect()
    }

    /// Records a position return at a specific timestamp.
    #[pyo3(name = "add_position_return")]
    fn py_add_position_return(&mut self, timestamp: u64, value: f64) {
        self.add_position_return(UnixNanos::from(timestamp), value);
    }

    /// Records a return at a specific timestamp.
    ///
    /// This is a backward-compatible alias for `Self.add_position_return`.
    #[pyo3(name = "add_return")]
    fn py_add_return(&mut self, timestamp: u64, value: f64) {
        self.add_return(UnixNanos::from(timestamp), value);
    }

    /// Resets all analysis data to initial state.
    ///
    /// Registered statistics are retained; use `Self.deregister_statistics` to clear them.
    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    /// Registers a new portfolio statistic for calculation.
    #[pyo3(name = "register_statistic")]
    fn py_register_statistic(&mut self, py: Python, statistic: Py<PyAny>) -> PyResult<()> {
        self.register_statistic(statistic_from_pyobject(py, statistic)?);
        Ok(())
    }

    /// Removes a specific statistic from calculation.
    #[pyo3(name = "deregister_statistic")]
    fn py_deregister_statistic(&mut self, py: Python, statistic: Py<PyAny>) -> PyResult<()> {
        self.deregister_statistic(&statistic_from_pyobject(py, statistic)?);
        Ok(())
    }

    /// Removes all registered statistics.
    #[pyo3(name = "deregister_statistics")]
    fn py_deregister_statistics(&mut self) {
        self.deregister_statistics();
    }

    /// Adds new positions for analysis.
    #[pyo3(name = "add_positions")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_positions(&mut self, py: Python, positions: Vec<Py<PyAny>>) -> PyResult<()> {
        let positions: Vec<Position> = positions
            .iter()
            .map(|position| position.extract::<Position>(py).map_err(Into::into))
            .collect::<PyResult<Vec<Position>>>()?;

        self.add_positions(&positions);
        Ok(())
    }

    /// Records a trade's PnL realized at `ts_event`.
    #[pyo3(name = "add_trade")]
    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "matches underlying add_trade signature"
    )]
    fn py_add_trade(&mut self, position_id: &PositionId, ts_event: u64, realized_pnl: &Money) {
        self.add_trade(position_id, UnixNanos::from(ts_event), realized_pnl);
    }

    /// Records a trade's PnL realized at `ts_event`, observed during portfolio processing.
    #[pyo3(name = "record_trade")]
    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "matches underlying record_trade signature"
    )]
    fn py_record_trade(&mut self, position_id: &PositionId, ts_event: u64, realized_pnl: &Money) {
        self.record_trade(position_id, UnixNanos::from(ts_event), realized_pnl);
    }

    // Note: calculate_statistics is not exposed to Python because it requires
    // complex conversions of Account and dict types. Use the Python analyzer.py wrapper instead.

    /// Retrieves a specific statistic by name.
    #[pyo3(name = "statistic")]
    fn py_statistic(&self, name: &str) -> Option<String> {
        self.statistic(name).map(|s| s.name())
    }

    /// Returns the primary calculated returns.
    ///
    /// This returns portfolio returns when available, otherwise it falls back
    /// to position returns for backward compatibility.
    #[pyo3(name = "returns")]
    fn py_returns(&self, py: Python) -> PyResult<Py<PyAny>> {
        // Convert BTreeMap<UnixNanos, f64> to Python dict
        let dict = pyo3::types::PyDict::new(py);
        for (timestamp, value) in self.returns() {
            dict.set_item(timestamp.as_u64(), value)?;
        }
        Ok(dict.into())
    }

    /// Returns the per-position calculated returns.
    #[pyo3(name = "position_returns")]
    fn py_position_returns(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = pyo3::types::PyDict::new(py);
        for (timestamp, value) in self.position_returns() {
            dict.set_item(timestamp.as_u64(), value)?;
        }
        Ok(dict.into())
    }

    /// Returns the portfolio calculated returns.
    #[pyo3(name = "portfolio_returns")]
    fn py_portfolio_returns(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = pyo3::types::PyDict::new(py);
        for (timestamp, value) in self.portfolio_returns() {
            dict.set_item(timestamp.as_u64(), value)?;
        }
        Ok(dict.into())
    }

    /// Retrieves realized PnLs for a specific currency.
    ///
    /// Each record is `(position_id, ts_event, realized_pnl)`, in ascending `ts_event` order.
    /// Returns `None` if no PnLs exist, or if multiple currencies exist without an explicit
    /// currency specified.
    #[pyo3(name = "realized_pnls")]
    fn py_realized_pnls(&self, py: Python, currency: Option<&Currency>) -> PyResult<Py<PyAny>> {
        match self.realized_pnls(currency) {
            Some(pnls) => {
                let list = pyo3::types::PyList::empty(py);
                for (position_id, ts_event, pnl) in pnls {
                    list.append((position_id.to_string(), ts_event.as_u64(), pnl))?;
                }
                Ok(list.into())
            }
            None => Ok(py.None()),
        }
    }

    /// Calculates total PnL including unrealized PnL if provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No currency is specified in a multi-currency portfolio.
    /// - The specified currency is not found in account balances.
    /// - The unrealized PnL currency does not match the specified currency.
    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> PyResult<f64> {
        self.total_pnl(currency, unrealized_pnl)
            .map_err(to_pyvalue_err)
    }

    /// Calculates total PnL as a percentage of starting balance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No currency is specified in a multi-currency portfolio.
    /// - The specified currency is not found in account balances.
    /// - The unrealized PnL currency does not match the specified currency.
    #[pyo3(name = "total_pnl_percentage")]
    fn py_total_pnl_percentage(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> PyResult<f64> {
        self.total_pnl_percentage(currency, unrealized_pnl)
            .map_err(to_pyvalue_err)
    }

    /// Gets formatted PnL statistics as strings.
    ///
    /// # Errors
    ///
    /// Returns an error if PnL statistics calculation fails.
    #[pyo3(name = "get_stats_pnls_formatted")]
    fn py_get_stats_pnls_formatted(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> PyResult<Vec<String>> {
        self.get_stats_pnls_formatted(currency, unrealized_pnl)
            .map_err(to_pyvalue_err)
    }

    /// Gets formatted return statistics as strings.
    #[pyo3(name = "get_stats_returns_formatted")]
    fn py_get_stats_returns_formatted(&self) -> Vec<String> {
        self.get_stats_returns_formatted()
    }

    /// Gets formatted position-return statistics as strings.
    #[pyo3(name = "get_stats_position_returns_formatted")]
    fn py_get_stats_position_returns_formatted(&self) -> Vec<String> {
        self.get_stats_position_returns_formatted()
    }

    /// Gets formatted portfolio-return statistics as strings.
    #[pyo3(name = "get_stats_portfolio_returns_formatted")]
    fn py_get_stats_portfolio_returns_formatted(&self) -> Vec<String> {
        self.get_stats_portfolio_returns_formatted()
    }

    /// Gets formatted general statistics as strings.
    #[pyo3(name = "get_stats_general_formatted")]
    fn py_get_stats_general_formatted(&self) -> Vec<String> {
        self.get_stats_general_formatted()
    }
}
