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

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.hyperliquid`.
#[pymodule]
pub fn hyperliquid(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::http::HyperliquidHttpClient>()?;
    m.add_class::<crate::websocket::HyperliquidWebSocketClient>()?;
    m.add_class::<crate::common::enums::HyperliquidTpSl>()?;
    m.add_class::<crate::common::enums::HyperliquidTriggerPriceType>()?;
    m.add_class::<crate::common::enums::HyperliquidConditionalOrderType>()?;
    m.add_class::<crate::common::enums::HyperliquidTrailingOffsetType>()?;
    m.add_function(wrap_pyfunction!(urls::get_hyperliquid_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::get_hyperliquid_ws_url, m)?)?;
    Ok(())
}
