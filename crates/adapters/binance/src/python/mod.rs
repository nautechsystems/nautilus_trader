// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
pub mod http_futures;
pub mod http_spot;
pub mod websocket_futures;
pub mod websocket_spot;

use pyo3::prelude::*;

use crate::{
    common::enums::{BinanceEnvironment, BinancePositionSide, BinanceProductType},
    futures::{
        http::{
            client::BinanceFuturesHttpClient,
            query::{
                BatchCancelItem as FuturesBatchCancelItem,
                BatchModifyItem as FuturesBatchModifyItem, BatchOrderItem as FuturesBatchOrderItem,
            },
        },
        websocket::client::BinanceFuturesWebSocketClient,
    },
    spot::{
        http::{
            client::BinanceSpotHttpClient,
            query::{BatchCancelItem as SpotBatchCancelItem, BatchOrderItem as SpotBatchOrderItem},
        },
        websocket::streams::client::BinanceSpotWebSocketClient,
    },
};

/// Binance adapter Python module.
///
/// Loaded as `nautilus_pyo3.binance`.
///
/// # Errors
///
/// Returns an error if module initialization fails.
#[pymodule]
pub fn binance(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<BinanceProductType>()?;
    m.add_class::<BinanceEnvironment>()?;
    m.add_class::<BinancePositionSide>()?;
    m.add_class::<BinanceSpotHttpClient>()?;
    m.add_class::<BinanceFuturesHttpClient>()?;
    m.add_class::<BinanceSpotWebSocketClient>()?;
    m.add_class::<BinanceFuturesWebSocketClient>()?;
    m.add_class::<FuturesBatchOrderItem>()?;
    m.add_class::<FuturesBatchCancelItem>()?;
    m.add_class::<FuturesBatchModifyItem>()?;
    m.add_class::<SpotBatchOrderItem>()?;
    m.add_class::<SpotBatchCancelItem>()?;

    Ok(())
}
