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
use crate::{statistic::PortfolioStatistic, statistics::sortino_ratio::SortinoRatio};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SortinoRatio {
    /// Calculates the Sortino ratio for portfolio returns.
    ///
    /// The Sortino ratio is a variation of the Sharpe ratio that only penalizes downside
    /// volatility, making it more appropriate for strategies with asymmetric return distributions.
    ///
    /// Formula: `Mean Return / Downside Deviation * sqrt(period)`
    ///
    /// Where downside deviation is calculated as:
    /// `sqrt(sum(negative_returns^2) / total_observations)`
    ///
    /// Note: Uses total observations count (not just negative returns) as per Sortino's methodology.
    ///
    /// # References
    ///
    /// - Sortino, F. A., & van der Meer, R. (1991). "Downside Risk". *Journal of Portfolio Management*, 17(4), 27-31.
    /// - Sortino, F. A., & Price, L. N. (1994). "Performance Measurement in a Downside Risk Framework".
    ///   *Journal of Investing*, 3(3), 59-64.
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
