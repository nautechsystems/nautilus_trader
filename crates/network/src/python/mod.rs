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

//! Python bindings for network configuration.

// We need to allow `unexpected_cfgs` because the PyO3 macros internally check for
// the `gil-refs` feature. We don't define or enable `gil-refs` ourselves (due to a
// memory leak), so the compiler raises an error about an unknown cfg feature.
// This attribute prevents those errors without actually enabling `gil-refs`.
#![allow(unexpected_cfgs)]
use pyo3::prelude::*;

/// Exposed through `nautilus_trader.network`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn network(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::websocket::TransportBackend>()?;

    Ok(())
}
