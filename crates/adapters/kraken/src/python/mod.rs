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

use pyo3::prelude::*;

use crate::{
    common::enums::{
        KrakenAssetClass, KrakenEnvironment, KrakenOrderSide, KrakenOrderStatus, KrakenOrderType,
        KrakenPairStatus, KrakenPositionSide, KrakenProductType, KrakenSystemStatus,
        KrakenTimeInForce,
    },
    http::client::KrakenHttpClient,
    websocket::{
        client::KrakenWebSocketClient,
        enums::{KrakenWsChannel, KrakenWsMessageType, KrakenWsMethod},
    },
};

pub mod enums;
pub mod http;
pub mod urls;
pub mod websocket;

#[pymodule]
pub fn kraken(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<KrakenEnvironment>()?;
    m.add_class::<KrakenProductType>()?;
    m.add_class::<KrakenOrderType>()?;
    m.add_class::<KrakenOrderSide>()?;
    m.add_class::<KrakenTimeInForce>()?;
    m.add_class::<KrakenOrderStatus>()?;
    m.add_class::<KrakenPositionSide>()?;
    m.add_class::<KrakenPairStatus>()?;
    m.add_class::<KrakenSystemStatus>()?;
    m.add_class::<KrakenAssetClass>()?;
    m.add_class::<KrakenWsMethod>()?;
    m.add_class::<KrakenWsChannel>()?;
    m.add_class::<KrakenWsMessageType>()?;

    m.add_class::<KrakenHttpClient>()?;
    m.add_class::<KrakenWebSocketClient>()?;

    m.add_function(wrap_pyfunction!(urls::py_get_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_ws_public_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_ws_private_url, m)?)?;

    Ok(())
}
