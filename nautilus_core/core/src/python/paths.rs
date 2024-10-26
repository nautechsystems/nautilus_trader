// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use pyo3::{prelude::*, types::PyTuple};

use crate::paths::{get_project_root_path, get_test_data_path, get_tests_root_path};

#[must_use]
#[pyfunction(name = "get_project_root_path")]
pub fn py_get_project_root_path() -> String {
    get_project_root_path()
        .as_os_str()
        .to_string_lossy()
        .to_string()
}

#[must_use]
#[pyfunction(name = "get_tests_root_path")]
pub fn py_get_tests_root_path() -> String {
    get_tests_root_path()
        .as_os_str()
        .to_string_lossy()
        .to_string()
}

#[must_use]
#[pyfunction(name = "get_test_data_path")]
pub fn py_get_test_data_path() -> String {
    get_test_data_path()
        .as_os_str()
        .to_string_lossy()
        .to_string()
}
