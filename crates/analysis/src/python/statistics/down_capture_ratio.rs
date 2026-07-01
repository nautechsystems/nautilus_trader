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
use crate::{statistic::PortfolioStatistic, statistics::down_capture_ratio::DownCaptureRatio};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DownCaptureRatio {
    /// Calculates the down capture ratio of portfolio returns relative to a benchmark.
    ///
    /// The down capture ratio measures how the portfolio performed, on average, during the
    /// periods when the benchmark return was negative. It is the ratio of the portfolio's
    /// geometric annualized return to the benchmark's geometric annualized return, both
    /// computed over the down-market subset only:
    ///
    /// `DownCapture = annualized_return(portfolio | benchmark < 0) / annualized_return(benchmark | benchmark < 0)`
    ///
    /// where each side's annualized return is the geometric (CAGR-style) value
    /// `(prod(1 + x_i))^(period / m) - 1` and `m` is the number of down-market periods. The
    /// period defaults to 252 trading days. This is the `empyrical.down_capture` convention
    /// (not the Morningstar cumulative-return ratio).
    ///
    /// # References
    ///
    /// - empyrical `down_capture` / `capture` / `annual_return`
    ///   (<https://github.com/quantopian/empyrical>).
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
