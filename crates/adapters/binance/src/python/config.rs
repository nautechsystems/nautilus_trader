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

use crate::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
};

#[pymethods]
impl BinanceDataClientConfig {
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        api_key = None,
        api_secret = None,
    ))]
    fn py_new(
        product_types: Option<Vec<BinanceProductType>>,
        environment: Option<BinanceEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> Self {
        Self {
            product_types: product_types.unwrap_or_else(|| vec![BinanceProductType::Spot]),
            environment: environment.unwrap_or(BinanceEnvironment::Mainnet),
            base_url_http,
            base_url_ws,
            api_key,
            api_secret,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl BinanceExecClientConfig {
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        product_types = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
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
            api_key,
            api_secret,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
