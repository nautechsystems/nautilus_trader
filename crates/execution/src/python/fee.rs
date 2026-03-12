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

//! Python bindings for fee model types.

use nautilus_core::python::to_pyruntime_err;
use nautilus_model::types::Money;
use pyo3::prelude::*;

use crate::models::fee::{FixedFeeModel, MakerTakerFeeModel, PerContractFeeModel};

#[pymethods]
impl FixedFeeModel {
    #[new]
    #[pyo3(signature = (commission, change_commission_once=None))]
    fn py_new(commission: Money, change_commission_once: Option<bool>) -> PyResult<Self> {
        Self::new(commission, change_commission_once).map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl MakerTakerFeeModel {
    #[new]
    fn py_new() -> Self {
        Self
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl PerContractFeeModel {
    #[new]
    fn py_new(commission: Money) -> PyResult<Self> {
        Self::new(commission).map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
