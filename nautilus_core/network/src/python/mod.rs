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

//! Python bindings from `pyo3`.

pub mod http;
pub mod socket;
pub mod websocket;

use pyo3::{prelude::*, PyTypeCheck};

use crate::python::{
    http::{HttpError, HttpTimeoutError},
    websocket::WebSocketClientError,
};

/// Loaded as nautilus_pyo3.network
#[pymodule]
pub fn network(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::http::HttpClient>()?;
    m.add_class::<crate::http::HttpMethod>()?;
    m.add_class::<crate::http::HttpResponse>()?;
    m.add_class::<crate::ratelimiter::quota::Quota>()?;
    m.add_class::<crate::websocket::WebSocketClient>()?;
    m.add_class::<crate::websocket::WebSocketConfig>()?;
    m.add_class::<crate::socket::SocketClient>()?;
    m.add_class::<crate::socket::SocketConfig>()?;

    // Add error classes
    m.add(
        <WebSocketClientError as PyTypeCheck>::NAME,
        m.py().get_type_bound::<WebSocketClientError>(),
    )?;
    m.add(
        <HttpError as PyTypeCheck>::NAME,
        m.py().get_type_bound::<HttpError>(),
    )?;
    m.add(
        <HttpTimeoutError as PyTypeCheck>::NAME,
        m.py().get_type_bound::<HttpTimeoutError>(),
    )?;

    Ok(())
}
