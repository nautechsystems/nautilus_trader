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

//! Python bindings for backtest node.

use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;

use crate::{config::BacktestRunConfig, engine::BacktestResult, node::BacktestNode};

#[pymethods]
impl BacktestNode {
    #[new]
    fn py_new(configs: Vec<BacktestRunConfig>) -> PyResult<Self> {
        Self::new(configs).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "build")]
    fn py_build(&mut self) -> PyResult<()> {
        self.build().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "run")]
    fn py_run(&mut self) -> PyResult<Vec<BacktestResult>> {
        self.run().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) {
        self.dispose();
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
