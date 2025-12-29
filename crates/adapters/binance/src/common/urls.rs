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

//! URL resolution helpers for Binance API endpoints.

use super::{
    consts::{
        BINANCE_FUTURES_COIN_HTTP_URL, BINANCE_FUTURES_COIN_TESTNET_HTTP_URL,
        BINANCE_FUTURES_COIN_TESTNET_WS_URL, BINANCE_FUTURES_COIN_WS_URL,
        BINANCE_FUTURES_USD_HTTP_URL, BINANCE_FUTURES_USD_TESTNET_HTTP_URL,
        BINANCE_FUTURES_USD_TESTNET_WS_URL, BINANCE_FUTURES_USD_WS_URL, BINANCE_OPTIONS_HTTP_URL,
        BINANCE_OPTIONS_WS_URL, BINANCE_SPOT_HTTP_URL, BINANCE_SPOT_TESTNET_HTTP_URL,
        BINANCE_SPOT_TESTNET_WS_URL, BINANCE_SPOT_WS_URL,
    },
    enums::{BinanceEnvironment, BinanceProductType},
};

/// Returns the HTTP base URL for the given product type and environment.
#[must_use]
pub fn get_http_base_url(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
) -> &'static str {
    match (product_type, environment) {
        // Mainnet
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Mainnet) => {
            BINANCE_SPOT_HTTP_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Mainnet) => BINANCE_FUTURES_USD_HTTP_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Mainnet) => BINANCE_FUTURES_COIN_HTTP_URL,
        (BinanceProductType::Options, BinanceEnvironment::Mainnet) => BINANCE_OPTIONS_HTTP_URL,

        // Testnet
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Testnet) => {
            BINANCE_SPOT_TESTNET_HTTP_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Testnet) => {
            BINANCE_FUTURES_USD_TESTNET_HTTP_URL
        }
        (BinanceProductType::CoinM, BinanceEnvironment::Testnet) => {
            BINANCE_FUTURES_COIN_TESTNET_HTTP_URL
        }
        // Options testnet not available, fall back to mainnet
        (BinanceProductType::Options, BinanceEnvironment::Testnet) => BINANCE_OPTIONS_HTTP_URL,
    }
}

/// Returns the WebSocket base URL for the given product type and environment.
#[must_use]
pub fn get_ws_base_url(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
) -> &'static str {
    match (product_type, environment) {
        // Mainnet
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Mainnet) => {
            BINANCE_SPOT_WS_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Mainnet) => BINANCE_FUTURES_USD_WS_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Mainnet) => BINANCE_FUTURES_COIN_WS_URL,
        (BinanceProductType::Options, BinanceEnvironment::Mainnet) => BINANCE_OPTIONS_WS_URL,

        // Testnet
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Testnet) => {
            BINANCE_SPOT_TESTNET_WS_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Testnet) => {
            BINANCE_FUTURES_USD_TESTNET_WS_URL
        }
        (BinanceProductType::CoinM, BinanceEnvironment::Testnet) => {
            BINANCE_FUTURES_COIN_TESTNET_WS_URL
        }
        // Options testnet not available, fall back to mainnet
        (BinanceProductType::Options, BinanceEnvironment::Testnet) => BINANCE_OPTIONS_WS_URL,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_http_url_spot_mainnet() {
        let url = get_http_base_url(BinanceProductType::Spot, BinanceEnvironment::Mainnet);
        assert_eq!(url, "https://api.binance.com");
    }

    #[rstest]
    fn test_http_url_spot_testnet() {
        let url = get_http_base_url(BinanceProductType::Spot, BinanceEnvironment::Testnet);
        assert_eq!(url, "https://testnet.binance.vision");
    }

    #[rstest]
    fn test_http_url_usdm_mainnet() {
        let url = get_http_base_url(BinanceProductType::UsdM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "https://fapi.binance.com");
    }

    #[rstest]
    fn test_http_url_coinm_mainnet() {
        let url = get_http_base_url(BinanceProductType::CoinM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "https://dapi.binance.com");
    }

    #[rstest]
    fn test_ws_url_spot_mainnet() {
        let url = get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Mainnet);
        assert_eq!(url, "wss://stream.binance.com:9443/ws");
    }

    #[rstest]
    fn test_ws_url_usdm_mainnet() {
        let url = get_ws_base_url(BinanceProductType::UsdM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "wss://fstream.binance.com/ws");
    }
}
