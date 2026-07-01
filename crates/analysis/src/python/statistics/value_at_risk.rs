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

use nautilus_core::python::to_pyvalue_err;
use pyo3::prelude::*;

use super::transform_returns;
use crate::{statistic::PortfolioStatistic, statistics::value_at_risk::ValueAtRisk};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ValueAtRisk {
    /// Calculates the historical Value at Risk (`VaR`) of portfolio returns.
    ///
    /// `VaR` is the loss threshold that returns are not expected to exceed at a given
    /// confidence level. This is the non-parametric (historical) estimator: the
    /// empirical quantile of the return distribution at `1 - confidence`.
    ///
    /// `VaR(c) = quantile(returns, 1 - c)`
    ///
    /// The quantile uses linear interpolation between closest ranks (matching
    /// `numpy.percentile`). `confidence` defaults to `0.95`. The result is expressed
    /// as a return (e.g. `-0.03` is a 3% loss threshold); more negative means greater
    /// risk. Returns `NaN` for an empty series.
    #[new]
    #[pyo3(signature = (confidence=None))]
    fn py_new(confidence: Option<f64>) -> PyResult<Self> {
        Self::new_checked(confidence).map_err(to_pyvalue_err)
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
