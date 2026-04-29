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

use std::collections::BTreeMap;

use pyo3::prelude::*;

use super::transform_returns;
use crate::{statistic::PortfolioStatistic, statistics::sharpe_ratio::SharpeRatio};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SharpeRatio {
    /// Calculates the Sharpe ratio for portfolio returns.
    ///
    /// The Sharpe ratio measures risk-adjusted return and is calculated as:
    /// `(Mean Return - Risk-free Rate) / Standard Deviation of Returns * sqrt(period)`
    ///
    /// This implementation assumes a risk-free rate of 0 and annualizes the ratio
    /// using the square root of the specified period (default: 252 trading days).
    ///
    /// # References
    ///
    /// - Sharpe, W. F. (1966). "Mutual Fund Performance". *Journal of Business*, 39(1), 119-138.
    /// - Sharpe, W. F. (1994). "The Sharpe Ratio". *Journal of Portfolio Management*, 21(1), 49-58.
    /// - CFA Institute Investment Foundations, 3rd Edition
    #[new]
    #[pyo3(signature = (period=None))]
    fn py_new(period: Option<usize>) -> Self {
        Self::new(period)
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[pyo3(name = "calculate_from_returns")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_calculate_from_returns(&mut self, raw_returns: BTreeMap<u64, f64>) -> Option<f64> {
        self.calculate_from_returns(&transform_returns(&raw_returns))
    }

    #[pyo3(name = "calculate_from_realized_pnls")]
    fn py_calculate_from_realized_pnls(&mut self, _realized_pnls: Vec<f64>) -> Option<f64> {
        None
    }

    #[pyo3(name = "calculate_from_positions")]
    fn py_calculate_from_positions(&mut self, _positions: Vec<Py<PyAny>>) -> Option<f64> {
        None
    }
}
