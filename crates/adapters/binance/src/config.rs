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

//! Binance adapter configuration structures.

use crate::common::enums::{BinanceEnvironment, BinanceProductType};

/// Configuration for Binance data client.
#[derive(Clone, Debug)]
pub struct BinanceDataClientConfig {
    /// Product types to subscribe to.
    pub product_types: Vec<BinanceProductType>,
    /// Environment (mainnet or testnet).
    pub environment: BinanceEnvironment,
    /// Optional base URL override for HTTP API.
    pub base_url_http: Option<String>,
    /// Optional base URL override for WebSocket.
    pub base_url_ws: Option<String>,
    /// API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// API secret for request signing.
    pub api_secret: Option<String>,
}

impl Default for BinanceDataClientConfig {
    fn default() -> Self {
        Self {
            product_types: vec![BinanceProductType::Spot],
            environment: BinanceEnvironment::Mainnet,
            base_url_http: None,
            base_url_ws: None,
            api_key: None,
            api_secret: None,
        }
    }
}

/// Configuration for Binance execution client.
#[derive(Clone, Debug)]
pub struct BinanceExecClientConfig {
    /// Product types to trade.
    pub product_types: Vec<BinanceProductType>,
    /// Environment (mainnet or testnet).
    pub environment: BinanceEnvironment,
    /// Optional base URL override for HTTP API.
    pub base_url_http: Option<String>,
    /// Optional base URL override for WebSocket.
    pub base_url_ws: Option<String>,
    /// API key for authenticated endpoints (required).
    pub api_key: String,
    /// API secret for request signing (required).
    pub api_secret: String,
}
