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

//! Python bindings for Delta Exchange adapter.

pub mod config;
pub mod error;
pub mod http;
pub mod models;
pub mod websocket;

use pyo3::prelude::*;

use self::{
    config::{PyDeltaExchangeHttpConfig, PyDeltaExchangeWsConfig},
    error::{register_exceptions, PyDeltaExchangeHttpError, PyDeltaExchangeWsError},
    http::PyDeltaExchangeHttpClient,
    models::{
        PyDeltaExchangeAsset, PyDeltaExchangeOrder, PyDeltaExchangePosition, PyDeltaExchangeProduct,
        PyDeltaExchangeTicker,
    },
    websocket::PyDeltaExchangeWebSocketClient,
};

/// Loaded as `nautilus_pyo3.delta_exchange`.
#[pymodule]
/// # Errors
///
/// Returns a Python exception if module initialization fails.
pub fn delta_exchange(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register exception types
    register_exceptions(py, m)?;

    // Configuration classes
    m.add_class::<PyDeltaExchangeHttpConfig>()?;
    m.add_class::<PyDeltaExchangeWsConfig>()?;

    // Error wrapper classes
    m.add_class::<PyDeltaExchangeHttpError>()?;
    m.add_class::<PyDeltaExchangeWsError>()?;

    // Client classes
    m.add_class::<PyDeltaExchangeHttpClient>()?;
    m.add_class::<PyDeltaExchangeWebSocketClient>()?;

    // Data model classes
    m.add_class::<PyDeltaExchangeAsset>()?;
    m.add_class::<PyDeltaExchangeProduct>()?;
    m.add_class::<PyDeltaExchangeTicker>()?;
    m.add_class::<PyDeltaExchangeOrder>()?;
    m.add_class::<PyDeltaExchangePosition>()?;

    // Module metadata
    m.add("__version__", "0.1.0")?;
    m.add("__author__", "Nautech Systems Pty Ltd")?;
    m.add("__email__", "info@nautechsystems.io")?;

    Ok(())
}

use pyo3::prelude::*;

pub mod http;
pub mod websocket;

/// Loaded as nautilus_trader.adapters.delta_exchange
#[pymodule]
pub fn delta_exchange(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<http::DeltaExchangeHttpClient>()?;
    m.add_class::<websocket::DeltaExchangeWebSocketClient>()?;
    Ok(())
}
