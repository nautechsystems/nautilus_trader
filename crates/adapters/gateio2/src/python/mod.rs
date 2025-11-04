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

//! Python bindings for Gate.io adapter.

use pyo3::prelude::*;

pub mod enums;
pub mod http;
pub mod websocket;

/// Registers the `gateio2` Python module.
#[pymodule]
pub fn gateio2(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<enums::PyGateioMarketType>()?;
    m.add_class::<enums::PyGateioOrderSide>()?;
    m.add_class::<enums::PyGateioOrderType>()?;
    m.add_class::<enums::PyGateioTimeInForce>()?;
    m.add_class::<enums::PyGateioOrderStatus>()?;
    m.add_class::<enums::PyGateioAccountType>()?;
    m.add_class::<http::PyGateioHttpClient>()?;
    m.add_class::<websocket::PyGateioWebSocketClient>()?;
    Ok(())
}
