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

use std::path::Path;

use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;

use crate::files::ensure_file_exists_or_download_http;

/// Python wrapper for `ensure_file_exists_or_download_http`.
///
/// Ensures that a file exists at the specified path by downloading it if necessary.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request fails or returns a non-success status code: `PyErr`.
/// - Any I/O operation fails during file creation, reading, or writing: `PyErr`.
/// - Checksum verification or JSON parsing fails: `PyErr`.
#[pyfunction(name = "ensure_file_exists_or_download_http")]
#[pyo3(signature = (filepath, url, checksums=None, timeout_secs=30))]
pub fn py_ensure_file_exists_or_download_http(
    filepath: &str,
    url: &str,
    checksums: Option<&str>,
    timeout_secs: Option<u64>,
) -> PyResult<()> {
    let filepath = Path::new(filepath);
    let checksums = checksums.map(Path::new);
    ensure_file_exists_or_download_http(filepath, url, checksums, timeout_secs)
        .map_err(to_pyruntime_err)
}
