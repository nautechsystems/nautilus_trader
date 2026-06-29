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
use crate::{statistic::PortfolioStatistic, statistics::omega_ratio::OmegaRatio};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OmegaRatio {
    /// Calculates the Omega ratio of portfolio returns.
    ///
    /// The Omega ratio is the ratio of probability-weighted gains to losses relative
    /// to a return threshold `θ`. It captures the entire return distribution (all
    /// moments), unlike the Sharpe ratio which only uses the first two:
    ///
    /// `Omega(θ) = sum(max(r - θ, 0)) / sum(max(θ - r, 0))`
    ///
    /// The threshold `θ` defaults to `0` (gains vs losses about zero). A value above
    /// `1` means gains above the threshold outweigh losses below it. Returns `NaN`
    /// for an empty series, or when there are no returns below the threshold (the
    /// ratio is undefined).
    ///
    /// # References
    ///
    /// - Keating, C., & Shadwick, W. F. (2002). "A Universal Performance Measure".
    ///   *Journal of Performance Measurement*, 6(3), 59-84.
    #[new]
    #[pyo3(signature = (threshold=None))]
    fn py_new(threshold: Option<f64>) -> Self {
        Self::new(threshold)
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
