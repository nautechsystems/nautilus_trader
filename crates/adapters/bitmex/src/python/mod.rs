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

pub mod canceller;
pub mod enums;
pub mod http;
pub mod urls;
pub mod websocket;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.bitmex`.
///
/// # Errors
///
/// Returns an error if the module registration fails or if adding functions/classes fails.
#[pymodule]
pub fn bitmex(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("BITMEX_HTTP_URL", crate::common::consts::BITMEX_HTTP_URL)?;
    m.add("BITMEX_WS_URL", crate::common::consts::BITMEX_WS_URL)?;
    m.add_class::<crate::common::enums::BitmexSymbolStatus>()?;
    m.add_class::<crate::common::enums::BitmexPositionSide>()?;
    m.add_class::<crate::http::client::BitmexHttpClient>()?;
    m.add_class::<crate::websocket::BitmexWebSocketClient>()?;
    m.add_class::<crate::execution::canceller::CancelBroadcaster>()?;
    m.add_function(wrap_pyfunction!(urls::get_bitmex_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::get_bitmex_ws_url, m)?)?;

    Ok(())
}
