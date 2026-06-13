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
use crate::{statistic::PortfolioStatistic, statistics::alpha::Alpha};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Alpha {
    /// Calculates Jensen's alpha of portfolio returns relative to a benchmark.
    ///
    /// Alpha measures the excess return of a portfolio over the return predicted by its
    /// beta exposure to the benchmark (CAPM). The per-period alpha is:
    ///
    /// `alpha = (mean_portfolio - rf) - beta * (mean_benchmark - rf)`
    ///
    /// where `beta` is the sample (`ddof = 1`) beta of the portfolio against the benchmark.
    /// The per-period alpha is then annualized geometrically over `period` (default 252):
    ///
    /// `alpha_annual = (1 + alpha)^period - 1`
    ///
    /// The risk-free rate `rf` is specified per period (default 0.0).
    ///
    /// # References
    ///
    /// - Jensen, M. C. (1968). "The Performance of Mutual Funds in the Period 1945-1964".
    ///   *Journal of Finance*, 23(2), 389-416.
    /// - CFA Institute Investment Foundations, 3rd Edition
    #[new]
    #[pyo3(signature = (period=None, risk_free_rate=None))]
    fn py_new(period: Option<usize>, risk_free_rate: Option<f64>) -> Self {
        Self::new(period, risk_free_rate)
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
    fn py_calculate_from_returns(&self, _returns: BTreeMap<u64, f64>) -> Option<f64> {
        None
    }

    #[pyo3(name = "calculate_from_realized_pnls")]
    fn py_calculate_from_realized_pnls(&self, _realized_pnls: Vec<f64>) -> Option<f64> {
        None
    }

    #[pyo3(name = "calculate_from_positions")]
    fn py_calculate_from_positions(&self, _positions: Vec<Py<PyAny>>) -> Option<f64> {
        None
    }

    #[pyo3(name = "calculate_from_returns_with_benchmark")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_calculate_from_returns_with_benchmark(
        &self,
        returns: BTreeMap<u64, f64>,
        benchmark: BTreeMap<u64, f64>,
    ) -> Option<f64> {
        self.calculate_from_returns_with_benchmark(
            &transform_returns(&returns),
            &transform_returns(&benchmark),
        )
    }
}
