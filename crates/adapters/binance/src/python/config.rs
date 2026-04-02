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

//! Python bindings for Binance configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;
use rust_decimal::Decimal;

use crate::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceDataClientConfig {
    /// Configuration for Binance data client.
    ///
    /// Ed25519 API keys are required for SBE WebSocket streams.
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        api_key = None,
        api_secret = None,
        instrument_status_poll_secs = None,
    ))]
    fn py_new(
        product_types: Option<Vec<BinanceProductType>>,
        environment: Option<BinanceEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        instrument_status_poll_secs: Option<u64>,
    ) -> Self {
        Self {
            product_types: product_types.unwrap_or_else(|| vec![BinanceProductType::Spot]),
            environment: environment.unwrap_or(BinanceEnvironment::Mainnet),
            base_url_http,
            base_url_ws,
            api_key,
            api_secret,
            instrument_status_poll_secs: instrument_status_poll_secs.unwrap_or(3600),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceExecClientConfig {
    /// Configuration for Binance execution client.
    ///
    /// Ed25519 API keys are required for execution clients. Binance deprecated
    /// listenKey-based user data streams in favor of WebSocket API authentication,
    /// which only supports Ed25519.
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        product_types = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        base_url_ws_trading = None,
        use_ws_trading = true,
        use_position_ids = true,
        default_taker_fee = None,
        api_key = None,
        api_secret = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        product_types: Option<Vec<BinanceProductType>>,
        environment: Option<BinanceEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        base_url_ws_trading: Option<String>,
        use_ws_trading: bool,
        use_position_ids: bool,
        default_taker_fee: Option<f64>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            product_types: product_types.unwrap_or_else(|| vec![BinanceProductType::Spot]),
            environment: environment.unwrap_or(BinanceEnvironment::Mainnet),
            base_url_http,
            base_url_ws,
            base_url_ws_trading,
            use_ws_trading,
            use_position_ids,
            default_taker_fee: default_taker_fee
                .map_or_else(|| Ok(Decimal::new(4, 4)), Decimal::try_from)
                .unwrap_or_else(|_| Decimal::new(4, 4)),
            api_key,
            api_secret,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
