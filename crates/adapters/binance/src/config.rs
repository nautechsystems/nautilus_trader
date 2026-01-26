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

//! Binance adapter configuration structures.

use std::any::Any;

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_system::factories::ClientConfig;

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
    /// API key for HTTP authenticated endpoints (HMAC).
    pub api_key: Option<String>,
    /// API secret for HTTP request signing (HMAC).
    pub api_secret: Option<String>,
    /// Ed25519 API key for SBE WebSocket streams (required for SBE).
    pub ed25519_api_key: Option<String>,
    /// Ed25519 private key (base64) for SBE WebSocket streams (required for SBE).
    pub ed25519_api_secret: Option<String>,
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
            ed25519_api_key: None,
            ed25519_api_secret: None,
        }
    }
}

impl ClientConfig for BinanceDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Configuration for Binance execution client.
#[derive(Clone, Debug)]
pub struct BinanceExecClientConfig {
    /// Trader ID for the client.
    pub trader_id: TraderId,
    /// Account ID for the client.
    pub account_id: AccountId,
    /// Product types to trade.
    pub product_types: Vec<BinanceProductType>,
    /// Environment (mainnet or testnet).
    pub environment: BinanceEnvironment,
    /// Optional base URL override for HTTP API.
    pub base_url_http: Option<String>,
    /// Optional base URL override for WebSocket.
    pub base_url_ws: Option<String>,
    /// API key for authenticated endpoints (optional, uses env var if not provided).
    pub api_key: Option<String>,
    /// API secret for request signing (optional, uses env var if not provided).
    pub api_secret: Option<String>,
}

impl ClientConfig for BinanceExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
