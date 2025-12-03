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

//! Python-specific runtime initialization.
//!
//! This module handles the Python interpreter initialization that must occur
//! before the Tokio runtime is used from Python extension modules.

use std::sync::Once;

use pyo3::Python;

static PYTHON_INIT: Once = Once::new();

/// Initializes the Python interpreter for use with the async runtime.
///
/// Python hosts the process when we build as an extension module. This function
/// keeps the interpreter alive for the lifetime of the shared Tokio runtime
/// so every worker thread sees a prepared PyO3 environment before using it.
///
/// This function is idempotent and safe to call multiple times.
pub fn initialize_python() {
    PYTHON_INIT.call_once(|| {
        Python::initialize();
    });
}
