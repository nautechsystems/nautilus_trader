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

//! Python bindings for the Binance adapter.

pub mod enums;
pub mod websocket;

use pyo3::prelude::*;

/// Binance adapter Python module.
///
/// Loaded as `nautilus_pyo3.binance`.
#[pymodule]
pub fn binance(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Enums
    m.add_class::<crate::common::enums::BinanceProductType>()?;
    m.add_class::<crate::common::enums::BinanceEnvironment>()?;

    // WebSocket clients
    m.add_class::<crate::spot::websocket::client::BinanceSpotWebSocketClient>()?;
    m.add_class::<crate::futures::websocket::client::BinanceFuturesWebSocketClient>()?;

    Ok(())
}
