// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_model::enums::OrderSide;
use pyo3::prelude::*;

use crate::{statistic::PortfolioStatistic, statistics::long_ratio::LongRatio};

#[pymethods]
impl LongRatio {
    #[new]
    #[pyo3(signature = (precision=None))]
    fn py_new(precision: Option<usize>) -> Self {
        Self::new(precision)
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[pyo3(name = "calculate_from_positions")]
    fn py_calculate_from_positions(
        &mut self,
        py: Python,
        positions: Vec<Py<PyAny>>,
    ) -> PyResult<Option<f64>> {
        if positions.is_empty() {
            return Ok(None);
        }

        // Extract entry side from each Cython Position object
        // OrderSide.Buy has value 1 in both Cython and Rust
        let mut longs = 0;
        for position in &positions {
            let entry = position.getattr(py, "entry")?;
            let entry_value: u8 = entry.extract(py)?;
            if entry_value == OrderSide::Buy as u8 {
                longs += 1;
            }
        }

        let value = f64::from(longs) / positions.len() as f64;
        let scale = 10f64.powi(self.precision as i32);
        Ok(Some((value * scale).round() / scale))
    }

    #[pyo3(name = "calculate_from_realized_pnls")]
    fn py_calculate_from_realized_pnls(&mut self, _realized_pnls: Vec<f64>) -> Option<f64> {
        None
    }

    #[pyo3(name = "calculate_from_returns")]
    #[allow(unused_variables)] // Pattern preserved for consistency across statistics
    fn py_calculate_from_returns(&mut self, _returns: BTreeMap<u64, f64>) -> Option<f64> {
        None
    }
}
