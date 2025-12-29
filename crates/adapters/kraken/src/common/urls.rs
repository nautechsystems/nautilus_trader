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

//! URL builders for Kraken HTTP and WebSocket endpoints.

use super::{
    consts::{
        KRAKEN_FUTURES_DEMO_HTTP_URL, KRAKEN_FUTURES_DEMO_WS_URL, KRAKEN_FUTURES_HTTP_URL,
        KRAKEN_FUTURES_WS_URL, KRAKEN_SPOT_HTTP_URL, KRAKEN_SPOT_WS_PRIVATE_URL,
        KRAKEN_SPOT_WS_PUBLIC_URL,
    },
    enums::{KrakenEnvironment, KrakenProductType},
};

/// Returns the HTTP base URL for the given product type and environment.
pub fn get_kraken_http_base_url(
    product_type: KrakenProductType,
    environment: KrakenEnvironment,
) -> &'static str {
    match (product_type, environment) {
        (KrakenProductType::Spot, _) => KRAKEN_SPOT_HTTP_URL,
        (KrakenProductType::Futures, KrakenEnvironment::Mainnet) => KRAKEN_FUTURES_HTTP_URL,
        (KrakenProductType::Futures, KrakenEnvironment::Demo) => KRAKEN_FUTURES_DEMO_HTTP_URL,
    }
}

/// Returns the public WebSocket URL for the given product type and environment.
pub fn get_kraken_ws_public_url(
    product_type: KrakenProductType,
    environment: KrakenEnvironment,
) -> &'static str {
    match (product_type, environment) {
        (KrakenProductType::Spot, _) => KRAKEN_SPOT_WS_PUBLIC_URL,
        (KrakenProductType::Futures, KrakenEnvironment::Mainnet) => KRAKEN_FUTURES_WS_URL,
        (KrakenProductType::Futures, KrakenEnvironment::Demo) => KRAKEN_FUTURES_DEMO_WS_URL,
    }
}

/// Returns the private WebSocket URL for the given product type and environment.
pub fn get_kraken_ws_private_url(
    product_type: KrakenProductType,
    environment: KrakenEnvironment,
) -> &'static str {
    match (product_type, environment) {
        (KrakenProductType::Spot, _) => KRAKEN_SPOT_WS_PRIVATE_URL,
        (KrakenProductType::Futures, KrakenEnvironment::Mainnet) => KRAKEN_FUTURES_WS_URL,
        (KrakenProductType::Futures, KrakenEnvironment::Demo) => KRAKEN_FUTURES_DEMO_WS_URL,
    }
}
