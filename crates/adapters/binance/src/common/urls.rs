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

//! URL resolution helpers for Binance API endpoints.

use super::{
    consts::{
        BINANCE_FUTURES_COIN_DEMO_HTTP_URL, BINANCE_FUTURES_COIN_HTTP_URL,
        BINANCE_FUTURES_COIN_TESTNET_HTTP_URL, BINANCE_FUTURES_COIN_TESTNET_WS_URL,
        BINANCE_FUTURES_COIN_WS_URL, BINANCE_FUTURES_USD_DEMO_HTTP_URL,
        BINANCE_FUTURES_USD_HTTP_URL, BINANCE_FUTURES_USD_TESTNET_HTTP_URL,
        BINANCE_FUTURES_USD_TESTNET_WS_URL, BINANCE_FUTURES_USD_WS_PRIVATE_URL,
        BINANCE_FUTURES_USD_WS_PUBLIC_URL, BINANCE_FUTURES_USD_WS_URL, BINANCE_OPTIONS_HTTP_URL,
        BINANCE_OPTIONS_WS_URL, BINANCE_SPOT_DEMO_HTTP_URL, BINANCE_SPOT_DEMO_WS_URL,
        BINANCE_SPOT_HTTP_URL, BINANCE_SPOT_TESTNET_HTTP_URL, BINANCE_SPOT_TESTNET_WS_URL,
        BINANCE_SPOT_WS_URL,
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
        (BinanceProductType::Options, BinanceEnvironment::Testnet) => BINANCE_OPTIONS_HTTP_URL,

        // Demo
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Demo) => {
            BINANCE_SPOT_DEMO_HTTP_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Demo) => BINANCE_FUTURES_USD_DEMO_HTTP_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Demo) => BINANCE_FUTURES_COIN_DEMO_HTTP_URL,
        (BinanceProductType::Options, BinanceEnvironment::Demo) => BINANCE_OPTIONS_HTTP_URL,
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
        (BinanceProductType::Options, BinanceEnvironment::Testnet) => BINANCE_OPTIONS_WS_URL,

        // Demo (futures demo uses same WS URLs as futures testnet)
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Demo) => {
            BINANCE_SPOT_DEMO_WS_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Demo) => BINANCE_FUTURES_USD_TESTNET_WS_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Demo) => {
            BINANCE_FUTURES_COIN_TESTNET_WS_URL
        }
        (BinanceProductType::Options, BinanceEnvironment::Demo) => BINANCE_OPTIONS_WS_URL,
    }
}

/// Returns the WebSocket public stream base URL for high-frequency book data.
///
/// USD-M mainnet uses the dedicated public endpoint for `@bookTicker` and
/// `@depth` streams. All other product types and environments fall back to
/// [`get_ws_base_url`].
#[must_use]
pub fn get_ws_public_base_url(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
) -> &'static str {
    match (product_type, environment) {
        (BinanceProductType::UsdM, BinanceEnvironment::Mainnet) => {
            BINANCE_FUTURES_USD_WS_PUBLIC_URL
        }
        _ => get_ws_base_url(product_type, environment),
    }
}

/// Returns the WebSocket private stream base URL for user data.
///
/// USD-M mainnet uses the dedicated private endpoint. All other
/// product types and environments fall back to [`get_ws_base_url`].
#[must_use]
pub fn get_ws_private_base_url(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
) -> &'static str {
    match (product_type, environment) {
        (BinanceProductType::UsdM, BinanceEnvironment::Mainnet) => {
            BINANCE_FUTURES_USD_WS_PRIVATE_URL
        }
        _ => get_ws_base_url(product_type, environment),
    }
}

fn is_usdm_ws_host(base_url: &str) -> bool {
    // Strip scheme (e.g. `wss://`) and trailing path/port, then match the hostname.
    // Accepts fstream.binance.com, fstream-mm.binance.com, fstream-auth.binance.com,
    // and their .us counterparts, without admitting arbitrary substrings.
    let without_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let host = without_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or(without_scheme);
    host.starts_with("fstream") && (host.ends_with(".binance.com") || host.ends_with(".binance.us"))
}

/// Returns a routed USD-M Futures WebSocket URL derived from an override.
///
/// Binance now routes USD-M Futures mainnet traffic by category. This helper
/// accepts either a root override (for example `wss://fstream.binance.com`) or
/// a routed/transport-specific override such as `/market`, `/public/ws`, or
/// `/private/stream`, then rebuilds the URL for the requested route.
///
/// URLs that do not point at `fstream.binance.com` (for example local test
/// endpoints) are returned unchanged.
#[must_use]
pub(crate) fn get_usdm_ws_route_base_url(base_url: &str, route: &str) -> String {
    const SUFFIXES: [&str; 11] = [
        "/market/ws",
        "/market/stream",
        "/public/ws",
        "/public/stream",
        "/private/ws",
        "/private/stream",
        "/market",
        "/public",
        "/private",
        "/ws",
        "/stream",
    ];

    assert!(
        matches!(route, "market" | "public" | "private"),
        "invalid USD-M WebSocket route: {route}"
    );

    if !is_usdm_ws_host(base_url) {
        return base_url.to_string();
    }

    let mut normalized = base_url.trim_end_matches('/').to_string();

    for suffix in SUFFIXES {
        if normalized.ends_with(suffix) {
            normalized.truncate(normalized.len() - suffix.len());
            break;
        }
    }

    format!("{normalized}/{route}/ws")
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
    fn test_http_url_spot_demo() {
        let url = get_http_base_url(BinanceProductType::Spot, BinanceEnvironment::Demo);
        assert_eq!(url, "https://demo-api.binance.com");
    }

    #[rstest]
    fn test_http_url_usdm_mainnet() {
        let url = get_http_base_url(BinanceProductType::UsdM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "https://fapi.binance.com");
    }

    #[rstest]
    fn test_http_url_usdm_testnet() {
        let url = get_http_base_url(BinanceProductType::UsdM, BinanceEnvironment::Testnet);
        assert_eq!(url, "https://demo-fapi.binance.com");
    }

    #[rstest]
    fn test_http_url_coinm_mainnet() {
        let url = get_http_base_url(BinanceProductType::CoinM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "https://dapi.binance.com");
    }

    #[rstest]
    fn test_http_url_usdm_demo() {
        let url = get_http_base_url(BinanceProductType::UsdM, BinanceEnvironment::Demo);
        assert_eq!(url, "https://demo-fapi.binance.com");
    }

    #[rstest]
    fn test_http_url_coinm_demo() {
        let url = get_http_base_url(BinanceProductType::CoinM, BinanceEnvironment::Demo);
        assert_eq!(url, "https://testnet.binancefuture.com");
    }

    #[rstest]
    fn test_ws_url_spot_mainnet() {
        let url = get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Mainnet);
        assert_eq!(url, "wss://stream.binance.com:9443/ws");
    }

    #[rstest]
    fn test_ws_url_spot_demo() {
        let url = get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Demo);
        assert_eq!(url, "wss://demo-stream.binance.com/ws");
    }

    #[rstest]
    fn test_ws_url_usdm_mainnet() {
        let url = get_ws_base_url(BinanceProductType::UsdM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "wss://fstream.binance.com/market/ws");
    }

    #[rstest]
    fn test_ws_url_usdm_testnet() {
        let url = get_ws_base_url(BinanceProductType::UsdM, BinanceEnvironment::Testnet);
        assert_eq!(url, "wss://fstream.binancefuture.com/ws");
    }

    #[rstest]
    fn test_ws_private_url_usdm_mainnet() {
        let url = get_ws_private_base_url(BinanceProductType::UsdM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "wss://fstream.binance.com/private/ws");
    }

    #[rstest]
    fn test_ws_private_url_fallback_to_market() {
        let url = get_ws_private_base_url(BinanceProductType::Spot, BinanceEnvironment::Mainnet);
        assert_eq!(
            url,
            get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Mainnet)
        );
    }

    #[rstest]
    fn test_ws_public_url_usdm_mainnet() {
        let url = get_ws_public_base_url(BinanceProductType::UsdM, BinanceEnvironment::Mainnet);
        assert_eq!(url, "wss://fstream.binance.com/public/ws");
    }

    #[rstest]
    fn test_ws_public_url_fallback_to_market() {
        let url = get_ws_public_base_url(BinanceProductType::Spot, BinanceEnvironment::Mainnet);
        assert_eq!(
            url,
            get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Mainnet)
        );
    }

    #[rstest]
    #[case(
        "wss://fstream.binance.com",
        "market",
        "wss://fstream.binance.com/market/ws"
    )]
    #[case(
        "wss://fstream.binance.com/ws",
        "public",
        "wss://fstream.binance.com/public/ws"
    )]
    #[case(
        "wss://fstream.binance.com/market/ws",
        "private",
        "wss://fstream.binance.com/private/ws"
    )]
    #[case(
        "wss://fstream-mm.binance.com",
        "market",
        "wss://fstream-mm.binance.com/market/ws"
    )]
    #[case(
        "wss://fstream-mm.binance.com/ws",
        "public",
        "wss://fstream-mm.binance.com/public/ws"
    )]
    #[case(
        "wss://fstream-auth.binance.com/market/ws",
        "private",
        "wss://fstream-auth.binance.com/private/ws"
    )]
    #[case(
        "wss://fstream.binance.us",
        "market",
        "wss://fstream.binance.us/market/ws"
    )]
    fn test_usdm_ws_route_base_url_normalizes_override(
        #[case] base_url: &str,
        #[case] route: &str,
        #[case] expected: &str,
    ) {
        let url = get_usdm_ws_route_base_url(base_url, route);
        assert_eq!(url, expected);
    }

    #[rstest]
    #[case("ws://127.0.0.1:9999/ws", "market")]
    #[case("wss://other.example.com/private/ws", "private")]
    #[case("ws://localhost:8080", "public")]
    #[case("wss://other-fstream.binance.com.example.org/ws", "market")]
    #[case("wss://fstream.binance.com.example.org/ws", "market")]
    fn test_usdm_ws_route_base_url_passes_through_non_binance_host(
        #[case] base_url: &str,
        #[case] route: &str,
    ) {
        let url = get_usdm_ws_route_base_url(base_url, route);
        assert_eq!(url, base_url);
    }
}
