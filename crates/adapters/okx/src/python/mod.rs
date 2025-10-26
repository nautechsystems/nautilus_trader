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
pub mod models;
pub mod urls;
pub mod websocket;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.okx`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn okx(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<super::websocket::OKXWebSocketClient>()?;
    m.add_class::<super::websocket::messages::OKXWebSocketError>()?;
    m.add_class::<super::http::OKXHttpClient>()?;
    m.add_class::<crate::http::models::OKXBalanceDetail>()?;
    m.add_class::<crate::common::enums::OKXInstrumentType>()?;
    m.add_class::<crate::common::enums::OKXContractType>()?;
    m.add_class::<crate::common::enums::OKXMarginMode>()?;
    m.add_class::<crate::common::enums::OKXTradeMode>()?;
    m.add_class::<crate::common::enums::OKXOrderStatus>()?;
    m.add_class::<crate::common::enums::OKXPositionMode>()?;
    m.add_class::<crate::common::enums::OKXVipLevel>()?;
    m.add_class::<crate::common::urls::OKXEndpointType>()?;
    m.add_function(wrap_pyfunction!(urls::get_okx_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::get_okx_ws_url_public, m)?)?;
    m.add_function(wrap_pyfunction!(urls::get_okx_ws_url_private, m)?)?;
    m.add_function(wrap_pyfunction!(urls::get_okx_ws_url_business, m)?)?;
    m.add_function(wrap_pyfunction!(urls::okx_requires_authentication, m)?)?;
    Ok(())
}
