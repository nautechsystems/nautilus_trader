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
use crate::{statistic::PortfolioStatistic, statistics::up_capture_ratio::UpCaptureRatio};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UpCaptureRatio {
    /// Calculates the up capture ratio of portfolio returns relative to a benchmark.
    ///
    /// The up capture ratio measures how the portfolio performed, on average, during the
    /// periods when the benchmark return was positive. It is the ratio of the portfolio's
    /// geometric annualized return to the benchmark's geometric annualized return, both
    /// computed over the up-market subset only:
    ///
    /// `UpCapture = annualized_return(portfolio | benchmark > 0) / annualized_return(benchmark | benchmark > 0)`
    ///
    /// where each side's annualized return is the geometric (CAGR-style) value
    /// `(prod(1 + x_i))^(period / m) - 1` and `m` is the number of up-market periods (the
    /// size of the filtered subset, not the full aligned length). The period defaults to
    /// 252 trading days. A value above 1.0 means the portfolio outperformed the benchmark
    /// in up markets.
    ///
    /// This is the `empyrical.up_capture` convention (geometric annualized-return ratio over
    /// the `benchmark > 0` subset). Note that this differs from the Morningstar definition,
    /// which uses a ratio of *cumulative* (non-annualized) returns; the two coincide only
    /// when both subsets contain the same number of periods.
    ///
    /// # References
    ///
    /// - empyrical `up_capture` / `capture` / `annual_return`
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
