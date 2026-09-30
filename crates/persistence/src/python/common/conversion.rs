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

//! Python conversions shared by catalog and writer bindings.

use nautilus_core::{Params, python::params::pydict_to_params};
use pyo3::{prelude::*, types::PyDict};

/// Converts a Python catalog `params` dict to `Params`.
pub(crate) fn catalog_params_from_py(
    py: Python<'_>,
    params: Option<Py<PyDict>>,
) -> PyResult<Option<Params>> {
    params.map_or(Ok(None), |params| pydict_to_params(py, &params))
}
