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
    common::enums::{KrakenEnvironment, KrakenProductType},
    http::{KrakenFuturesHttpClient, KrakenSpotHttpClient},
    websocket::{
        futures::client::KrakenFuturesWebSocketClient, spot_v2::client::KrakenSpotWebSocketClient,
    },
};

pub mod enums;
pub mod http_futures;
pub mod http_spot;
pub mod websocket_futures;
pub mod websocket_spot;

/// Determines the product type from a Kraken symbol.
///
/// Futures symbols have the following prefixes:
/// - `PI_` - Perpetual Inverse futures (e.g., `PI_XBTUSD`)
/// - `PF_` - Perpetual Fixed-margin futures (e.g., `PF_XBTUSD`)
/// - `FI_` - Fixed maturity Inverse futures (e.g., `FI_XBTUSD_230929`)
/// - `FF_` - Flex futures
///
/// All other symbols are considered spot.
#[pyfunction]
#[pyo3(name = "kraken_product_type_from_symbol")]
fn py_kraken_product_type_from_symbol(symbol: &str) -> KrakenProductType {
    crate::common::enums::product_type_from_symbol(symbol)
}

#[pymodule]
pub fn kraken(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<KrakenEnvironment>()?;
    m.add_class::<KrakenProductType>()?;
    m.add_class::<KrakenSpotHttpClient>()?;
    m.add_class::<KrakenFuturesHttpClient>()?;
    m.add_class::<KrakenSpotWebSocketClient>()?;
    m.add_class::<KrakenFuturesWebSocketClient>()?;

    m.add_function(wrap_pyfunction!(py_kraken_product_type_from_symbol, m)?)?;

    Ok(())
}
