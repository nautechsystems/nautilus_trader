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

//! Python bindings for the Ax adapter.

pub mod http;
pub mod websocket;

use pyo3::prelude::*;

use crate::{
    common::enums::{AxEnvironment, AxMarketDataLevel},
    http::client::AxHttpClient,
    websocket::data::AxMdWebSocketClient,
};

/// Loaded as `nautilus_pyo3.architect`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn architect(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<AxEnvironment>()?;
    m.add_class::<AxMarketDataLevel>()?;
    m.add_class::<AxHttpClient>()?;
    m.add_class::<AxMdWebSocketClient>()?;

    Ok(())
}
