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

use std::{collections::HashMap, sync::Arc};

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use nautilus_model::{
    identifiers::PositionId,
    position::Position,
    types::{Currency, Money},
};
use pyo3::prelude::*;

use crate::{
    analyzer::{PortfolioAnalyzer, Statistic},
    statistics::{
        expectancy::Expectancy, long_ratio::LongRatio, loser_avg::AvgLoser, loser_max::MaxLoser,
        loser_min::MinLoser, profit_factor::ProfitFactor, returns_avg::ReturnsAverage,
        returns_avg_loss::ReturnsAverageLoss, returns_avg_win::ReturnsAverageWin,
        returns_volatility::ReturnsVolatility, risk_return_ratio::RiskReturnRatio,
        sharpe_ratio::SharpeRatio, sortino_ratio::SortinoRatio, win_rate::WinRate,
        winner_avg::AvgWinner, winner_max::MaxWinner, winner_min::MinWinner,
    },
};

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

    /// Calculates total PnL including unrealized PnL if provided.
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
    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    /// Registers a new portfolio statistic for calculation.
    #[pyo3(name = "register_statistic")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_register_statistic(&mut self, py: Python, statistic: Py<PyAny>) -> PyResult<()> {
        let type_name = statistic
            .getattr(py, "__class__")?
            .getattr(py, "__name__")?
            .extract::<String>(py)?;

        match type_name.as_str() {
            "MaxWinner" => {
                let stat = statistic.extract::<MaxWinner>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "MinWinner" => {
                let stat = statistic.extract::<MinWinner>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "AvgWinner" => {
                let stat = statistic.extract::<AvgWinner>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "MaxLoser" => {
                let stat = statistic.extract::<MaxLoser>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "MinLoser" => {
                let stat = statistic.extract::<MinLoser>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "AvgLoser" => {
                let stat = statistic.extract::<AvgLoser>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "Expectancy" => {
                let stat = statistic.extract::<Expectancy>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "WinRate" => {
                let stat = statistic.extract::<WinRate>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "ReturnsVolatility" => {
                let stat = statistic.extract::<ReturnsVolatility>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "ReturnsAverage" => {
                let stat = statistic.extract::<ReturnsAverage>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "ReturnsAverageLoss" => {
                let stat = statistic.extract::<ReturnsAverageLoss>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "ReturnsAverageWin" => {
                let stat = statistic.extract::<ReturnsAverageWin>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "SharpeRatio" => {
                let stat = statistic.extract::<SharpeRatio>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "SortinoRatio" => {
                let stat = statistic.extract::<SortinoRatio>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "ProfitFactor" => {
                let stat = statistic.extract::<ProfitFactor>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "RiskReturnRatio" => {
                let stat = statistic.extract::<RiskReturnRatio>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            "LongRatio" => {
                let stat = statistic.extract::<LongRatio>(py)?;
                self.register_statistic(Arc::new(stat));
            }
            _ => {
                return Err(to_pyvalue_err(format!(
                    "Unknown statistic type: {type_name}"
                )));
            }
        }

        Ok(())
    }

    /// Removes a specific statistic from calculation.
    #[pyo3(name = "deregister_statistic")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_deregister_statistic(&mut self, py: Python, statistic: Py<PyAny>) -> PyResult<()> {
        let type_name = statistic
            .getattr(py, "__class__")?
            .getattr(py, "__name__")?
            .extract::<String>(py)?;

        match type_name.as_str() {
            "MaxWinner" => {
                let stat = statistic.extract::<MaxWinner>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "MinWinner" => {
                let stat = statistic.extract::<MinWinner>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "AvgWinner" => {
                let stat = statistic.extract::<AvgWinner>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "MaxLoser" => {
                let stat = statistic.extract::<MaxLoser>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "MinLoser" => {
                let stat = statistic.extract::<MinLoser>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "AvgLoser" => {
                let stat = statistic.extract::<AvgLoser>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "Expectancy" => {
                let stat = statistic.extract::<Expectancy>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "WinRate" => {
                let stat = statistic.extract::<WinRate>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "ReturnsVolatility" => {
                let stat = statistic.extract::<ReturnsVolatility>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "ReturnsAverage" => {
                let stat = statistic.extract::<ReturnsAverage>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "ReturnsAverageLoss" => {
                let stat = statistic.extract::<ReturnsAverageLoss>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "ReturnsAverageWin" => {
                let stat = statistic.extract::<ReturnsAverageWin>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "SharpeRatio" => {
                let stat = statistic.extract::<SharpeRatio>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "SortinoRatio" => {
                let stat = statistic.extract::<SortinoRatio>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "ProfitFactor" => {
                let stat = statistic.extract::<ProfitFactor>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "RiskReturnRatio" => {
                let stat = statistic.extract::<RiskReturnRatio>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            "LongRatio" => {
                let stat = statistic.extract::<LongRatio>(py)?;
                self.deregister_statistic(&(Arc::new(stat) as Statistic));
            }
            _ => {
                return Err(to_pyvalue_err(format!(
                    "Unknown statistic type: {type_name}"
                )));
            }
        }

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
        // Extract Position objects from Cython wrappers
        let positions: Vec<Position> = positions
            .iter()
            .map(|p| {
                // Try to get the underlying Rust Position
                // For now, we'll need to handle Cython Position by accessing its _mem field
                p.getattr(py, "_mem")?
                    .extract::<Position>(py)
                    .map_err(Into::into)
            })
            .collect::<PyResult<Vec<Position>>>()?;

        self.add_positions(&positions);
        Ok(())
    }

    /// Records a trade's PnL.
    #[pyo3(name = "add_trade")]
    fn py_add_trade(&mut self, position_id: &PositionId, realized_pnl: &Money) {
        self.add_trade(position_id, realized_pnl);
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
    /// Returns `None` if no PnLs exist, or if multiple currencies exist
    /// without an explicit currency specified.
    #[pyo3(name = "realized_pnls")]
    fn py_realized_pnls(&self, py: Python, currency: Option<&Currency>) -> PyResult<Py<PyAny>> {
        match self.realized_pnls(currency) {
            Some(pnls) => {
                // Convert Vec<(PositionId, f64)> to Python list of tuples or dict
                let dict = pyo3::types::PyDict::new(py);
                for (position_id, pnl) in pnls {
                    dict.set_item(position_id.to_string(), pnl)?;
                }
                Ok(dict.into())
            }
            None => Ok(py.None()),
        }
    }

    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> PyResult<f64> {
        self.total_pnl(currency, unrealized_pnl)
            .map_err(to_pyvalue_err)
    }

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
