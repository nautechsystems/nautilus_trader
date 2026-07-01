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
use crate::{statistic::PortfolioStatistic, statistics::returns_skewness::ReturnsSkewness};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ReturnsSkewness {
    /// Calculates the skewness of portfolio returns.
    ///
    /// Skewness measures the asymmetry of the return distribution about its mean. A
    /// negative value indicates a longer left tail (downside outliers); a positive
    /// value indicates a longer right tail.
    ///
    /// Uses the bias-corrected sample skewness (adjusted Fisher-Pearson), matching
    /// `pandas.Series.skew` and Excel `SKEW`:
    ///
    /// `G1 = n / ((n - 1)(n - 2)) * sum(((x - mean) / s)^3)`
    ///
    /// where `s` is the sample standard deviation (Bessel's correction, ddof=1).
    /// Returns `NaN` for fewer than three returns or zero dispersion.
    ///
    /// # References
    ///
    /// - Joanes, D. N., & Gill, C. A. (1998). Comparing measures of sample skewness
    ///   and kurtosis. *Journal of the Royal Statistical Society: Series D*, 47(1), 183-189.
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
