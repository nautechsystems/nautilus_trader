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

//! Python bindings from `pyo3`.

pub mod enums;
pub mod http;
pub mod urls;
pub mod websocket;

use pyo3::prelude::*;

use crate::common::consts::BYBIT_NAUTILUS_BROKER_ID;

/// Loaded as `nautilus_pyo3.bybit`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
#[rustfmt::skip]
pub fn bybit(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(BYBIT_NAUTILUS_BROKER_ID), BYBIT_NAUTILUS_BROKER_ID)?;
    m.add_class::<crate::common::enums::BybitAccountType>()?;
    m.add_class::<crate::common::enums::BybitMarginMode>()?;
    m.add_class::<crate::common::enums::BybitPositionMode>()?;
    m.add_class::<crate::common::enums::BybitProductType>()?;
    m.add_class::<crate::common::enums::BybitEnvironment>()?;
    m.add_class::<crate::http::client::BybitHttpClient>()?;
    m.add_class::<crate::http::models::BybitTickerData>()?;
    m.add_class::<crate::websocket::client::BybitWebSocketClient>()?;
    m.add_class::<crate::websocket::messages::BybitWebSocketError>()?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_ws_url_public, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_ws_url_private, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_ws_url_trade, m)?)?;
    Ok(())
}
