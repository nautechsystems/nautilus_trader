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
use crate::{statistic::PortfolioStatistic, statistics::ulcer_index::UlcerIndex};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UlcerIndex {
    /// Calculates the Ulcer Index of portfolio returns.
    ///
    /// The Ulcer Index measures downside risk as the root-mean-square of the
    /// percentage drawdowns of the cumulative-return equity curve. Unlike volatility
    /// it only penalizes downside deviations, and unlike maximum drawdown it accounts
    /// for both the depth and the duration of drawdowns.
    ///
    /// The equity curve compounds returns from a starting value of `1.0`, and each
    /// drawdown is measured against the running peak (matching the convention used by
    /// `MaxDrawdown`):
    ///
    /// `UI = sqrt( mean( D_i^2 ) )`, where `D_i = (peak_i - equity_i) / peak_i`
    ///
    /// Drawdowns are expressed as fractions (`0.05` = 5%), so the result is on the
    /// same scale as `MaxDrawdown`. Returns `0.0` for an empty series.
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    fn __repr__(&self) -> String {
        self.name()
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
