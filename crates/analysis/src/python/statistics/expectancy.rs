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

#[allow(unused_imports)] // Used in template pattern for returns conversion
use nautilus_core::UnixNanos;
use pyo3::prelude::*;

use crate::{statistic::PortfolioStatistic, statistics::expectancy::Expectancy};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Expectancy {
    /// Calculates the expectancy of a trading strategy based on realized PnLs.
    ///
    /// Expectancy is defined as: `(Average Win × Win Rate) + (Average Loss × Loss Rate)`
    /// This metric provides insight into the expected profitability per trade and helps
    /// evaluate the overall edge of a trading strategy.
    ///
    /// A positive expectancy indicates a profitable system over time, while a negative
    /// expectancy suggests losses.
    ///
    /// # References
    ///
    /// - Tharp, V. K. (1998). *Trade Your Way to Financial Freedom*. McGraw-Hill.
    /// - Elder, A. (1993). *Trading for a Living*. John Wiley & Sons.
    /// - Vince, R. (1992). *The Mathematics of Money Management*. John Wiley & Sons.
    #[new]
    fn py_new() -> Self {
        Self {}
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[pyo3(name = "calculate_from_realized_pnls")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_calculate_from_realized_pnls(&mut self, realized_pnls: Vec<f64>) -> Option<f64> {
        self.calculate_from_realized_pnls(&realized_pnls)
    }

    #[pyo3(name = "calculate_from_returns")]
    #[allow(unused_variables)] // Pattern preserved for consistency across statistics
    fn py_calculate_from_returns(&mut self, _returns: BTreeMap<u64, f64>) -> Option<f64> {
        None
    }

    #[pyo3(name = "calculate_from_positions")]
    fn py_calculate_from_positions(&mut self, _positions: Vec<Py<PyAny>>) -> Option<f64> {
        None
    }
}
