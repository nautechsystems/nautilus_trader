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
        BINANCE_FUTURES_COIN_DEMO_HTTP_URL, BINANCE_FUTURES_COIN_DEMO_WS_URL,
        BINANCE_FUTURES_COIN_HTTP_URL, BINANCE_FUTURES_COIN_TESTNET_HTTP_URL,
        BINANCE_FUTURES_COIN_TESTNET_WS_URL, BINANCE_FUTURES_COIN_WS_URL,
        BINANCE_FUTURES_USD_DEMO_HTTP_URL, BINANCE_FUTURES_USD_DEMO_WS_URL,
        BINANCE_FUTURES_USD_HTTP_URL, BINANCE_FUTURES_USD_TESTNET_HTTP_URL,
        BINANCE_FUTURES_USD_TESTNET_WS_URL, BINANCE_FUTURES_USD_WS_PRIVATE_URL,
        BINANCE_FUTURES_USD_WS_PUBLIC_URL, BINANCE_FUTURES_USD_WS_URL, BINANCE_OPTIONS_HTTP_URL,
        BINANCE_OPTIONS_TESTNET_HTTP_URL, BINANCE_OPTIONS_TESTNET_WS_PRIVATE_URL,
        BINANCE_OPTIONS_TESTNET_WS_PUBLIC_URL, BINANCE_OPTIONS_TESTNET_WS_URL,
        BINANCE_OPTIONS_WS_URL, BINANCE_SPOT_DEMO_HTTP_URL, BINANCE_SPOT_DEMO_WS_URL,
        BINANCE_SPOT_HTTP_URL, BINANCE_SPOT_TESTNET_HTTP_URL, BINANCE_SPOT_TESTNET_WS_URL,
        BINANCE_SPOT_WS_URL, BINANCE_US_SPOT_HTTP_URL, BINANCE_US_SPOT_USER_WS_URL,
        BINANCE_US_SPOT_WS_URL,
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
        // Live
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Live) => {
            BINANCE_SPOT_HTTP_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Live) => BINANCE_FUTURES_USD_HTTP_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Live) => BINANCE_FUTURES_COIN_HTTP_URL,
        (BinanceProductType::Options, BinanceEnvironment::Live) => BINANCE_OPTIONS_HTTP_URL,

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
        (BinanceProductType::Options, BinanceEnvironment::Testnet) => {
            BINANCE_OPTIONS_TESTNET_HTTP_URL
        }

        // Demo
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Demo) => {
            BINANCE_SPOT_DEMO_HTTP_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Demo) => BINANCE_FUTURES_USD_DEMO_HTTP_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Demo) => BINANCE_FUTURES_COIN_DEMO_HTTP_URL,
        (BinanceProductType::Options, BinanceEnvironment::Demo) => BINANCE_OPTIONS_TESTNET_HTTP_URL,
    }
}

/// Returns the HTTP base URL, including first-class Binance US routing.
#[must_use]
pub fn get_http_base_url_with_us(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
    us: bool,
) -> &'static str {
    if us {
        BINANCE_US_SPOT_HTTP_URL
    } else {
        get_http_base_url(product_type, environment)
    }
}

/// Returns the SAPI base URL for the given environment, or `None` where SAPI is unavailable.
///
/// SAPI endpoints (`/sapi/v1/...`) are served from the Spot host on the live exchange. The
/// testnet and demo hosts do not route `/sapi/v1`, so callers must treat `None` as unavailable
/// rather than falling back to live, which would route account management against real funds.
#[must_use]
pub fn get_sapi_base_url(environment: BinanceEnvironment) -> Option<&'static str> {
    match environment {
        BinanceEnvironment::Live => Some(BINANCE_SPOT_HTTP_URL),
        BinanceEnvironment::Testnet | BinanceEnvironment::Demo => None,
    }
}

/// Returns the WebSocket base URL for the given product type and environment.
#[must_use]
pub fn get_ws_base_url(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
) -> &'static str {
    match (product_type, environment) {
        // Live
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Live) => {
            BINANCE_SPOT_WS_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Live) => BINANCE_FUTURES_USD_WS_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Live) => BINANCE_FUTURES_COIN_WS_URL,
        (BinanceProductType::Options, BinanceEnvironment::Live) => BINANCE_OPTIONS_WS_URL,

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
        (BinanceProductType::Options, BinanceEnvironment::Testnet) => {
            BINANCE_OPTIONS_TESTNET_WS_URL
        }

        // Demo
        (BinanceProductType::Spot | BinanceProductType::Margin, BinanceEnvironment::Demo) => {
            BINANCE_SPOT_DEMO_WS_URL
        }
        (BinanceProductType::UsdM, BinanceEnvironment::Demo) => BINANCE_FUTURES_USD_DEMO_WS_URL,
        (BinanceProductType::CoinM, BinanceEnvironment::Demo) => BINANCE_FUTURES_COIN_DEMO_WS_URL,
        (BinanceProductType::Options, BinanceEnvironment::Demo) => BINANCE_OPTIONS_TESTNET_WS_URL,
    }
}

/// Returns the WebSocket base URL, including first-class Binance US routing.
#[must_use]
pub fn get_ws_base_url_with_us(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
    us: bool,
) -> &'static str {
    if us {
        BINANCE_US_SPOT_WS_URL
    } else {
        get_ws_base_url(product_type, environment)
    }
}

/// Returns a Spot user stream URL bound to the supplied listen key.
#[must_use]
pub(crate) fn get_spot_user_stream_url(base_url: Option<&str>, listen_key: &str) -> String {
    let base_url = base_url.unwrap_or(BINANCE_US_SPOT_USER_WS_URL);
    let normalized = base_url.trim_end_matches('/');
    if normalized.ends_with("/ws") {
        format!("{normalized}/{listen_key}")
    } else {
        format!("{normalized}/ws/{listen_key}")
    }
}

/// Returns the WebSocket public stream base URL for high-frequency book data.
///
/// USD-M live exchange uses the dedicated public endpoint for `@bookTicker` and
/// `@depth` streams. All other product types and environments fall back to
/// [`get_ws_base_url`].
#[must_use]
pub fn get_ws_public_base_url(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
) -> &'static str {
    match (product_type, environment) {
        (BinanceProductType::UsdM, BinanceEnvironment::Live) => BINANCE_FUTURES_USD_WS_PUBLIC_URL,
        (BinanceProductType::Options, BinanceEnvironment::Testnet | BinanceEnvironment::Demo) => {
            BINANCE_OPTIONS_TESTNET_WS_PUBLIC_URL
        }
        _ => get_ws_base_url(product_type, environment),
    }
}

/// Returns the WebSocket private stream base URL for user data.
///
/// USD-M live exchange uses the dedicated private endpoint. All other
/// product types and environments fall back to [`get_ws_base_url`].
#[must_use]
pub fn get_ws_private_base_url(
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
) -> &'static str {
    match (product_type, environment) {
        (BinanceProductType::UsdM, BinanceEnvironment::Live) => BINANCE_FUTURES_USD_WS_PRIVATE_URL,
        (BinanceProductType::Options, BinanceEnvironment::Testnet | BinanceEnvironment::Demo) => {
            BINANCE_OPTIONS_TESTNET_WS_PRIVATE_URL
        }
        _ => get_ws_base_url(product_type, environment),
    }
}

/// Returns a Futures user stream URL bound to the supplied listen key.
#[must_use]
pub(crate) fn get_futures_user_stream_url(
    product_type: BinanceProductType,
    base_url: &str,
    listen_key: &str,
) -> String {
    assert!(
        matches!(
            product_type,
            BinanceProductType::UsdM | BinanceProductType::CoinM
        ),
        "Futures user stream requires UsdM or CoinM product type, was {product_type:?}"
    );

    let mut normalized = base_url.trim_end_matches('/').to_string();
    let path = normalized
        .split_once("://")
        .map_or(normalized.as_str(), |(_, rest)| rest)
        .split_once('/')
        .map(|(_, path)| path);
    if matches!(path, None | Some("" | "private")) {
        normalized.push_str("/ws");
    }

    match product_type {
        BinanceProductType::UsdM => format!("{normalized}?listenKey={listen_key}"),
        BinanceProductType::CoinM => format!("{normalized}/{listen_key}"),
        _ => unreachable!(),
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
/// Binance now routes USD-M Futures live traffic by category. This helper
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
    fn test_http_url_spot_live() {
        let url = get_http_base_url(BinanceProductType::Spot, BinanceEnvironment::Live);
        assert_eq!(url, "https://api.binance.com");
    }

    #[rstest]
    fn test_binance_us_spot_urls() {
        let http =
            get_http_base_url_with_us(BinanceProductType::Spot, BinanceEnvironment::Live, true);
        let public_ws =
            get_ws_base_url_with_us(BinanceProductType::Spot, BinanceEnvironment::Live, true);
        let user_ws = get_spot_user_stream_url(None, "listen-key");

        assert_eq!(http, "https://api.binance.us");
        assert_eq!(public_ws, "wss://stream.binance.us:9443/ws");
        assert_eq!(user_ws, "wss://stream.binance.us:443/ws/listen-key");
    }

    #[rstest]
    #[case("wss://custom.example/ws", "wss://custom.example/ws/listen-key")]
    #[case("wss://custom.example", "wss://custom.example/ws/listen-key")]
    #[case("wss://custom.example/ws/", "wss://custom.example/ws/listen-key")]
    fn test_spot_user_stream_url_override(#[case] base: &str, #[case] expected: &str) {
        assert_eq!(get_spot_user_stream_url(Some(base), "listen-key"), expected);
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
    fn test_http_url_usdm_live() {
        let url = get_http_base_url(BinanceProductType::UsdM, BinanceEnvironment::Live);
        assert_eq!(url, "https://fapi.binance.com");
    }

    #[rstest]
    fn test_http_url_usdm_testnet() {
        let url = get_http_base_url(BinanceProductType::UsdM, BinanceEnvironment::Testnet);
        assert_eq!(url, "https://demo-fapi.binance.com");
    }

    #[rstest]
    fn test_http_url_coinm_live() {
        let url = get_http_base_url(BinanceProductType::CoinM, BinanceEnvironment::Live);
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
        assert_eq!(url, "https://demo-dapi.binance.com");
    }

    #[rstest]
    fn test_http_url_options_testnet() {
        let url = get_http_base_url(BinanceProductType::Options, BinanceEnvironment::Testnet);
        assert_eq!(url, "https://testnet.binancefuture.com");
    }

    #[rstest]
    fn test_http_url_options_demo() {
        let url = get_http_base_url(BinanceProductType::Options, BinanceEnvironment::Demo);
        assert_eq!(url, "https://testnet.binancefuture.com");
    }

    #[rstest]
    fn test_ws_url_spot_live() {
        let url = get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Live);
        assert_eq!(url, "wss://stream.binance.com:9443/ws");
    }

    #[rstest]
    fn test_ws_url_spot_demo() {
        let url = get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Demo);
        assert_eq!(url, "wss://demo-stream.binance.com/ws");
    }

    #[rstest]
    fn test_ws_url_usdm_live() {
        let url = get_ws_base_url(BinanceProductType::UsdM, BinanceEnvironment::Live);
        assert_eq!(url, "wss://fstream.binance.com/market/ws");
    }

    #[rstest]
    fn test_ws_url_usdm_testnet() {
        let url = get_ws_base_url(BinanceProductType::UsdM, BinanceEnvironment::Testnet);
        assert_eq!(url, "wss://fstream.binancefuture.com/ws");
    }

    #[rstest]
    fn test_ws_url_usdm_demo() {
        let url = get_ws_base_url(BinanceProductType::UsdM, BinanceEnvironment::Demo);
        assert_eq!(url, "wss://demo-fstream.binance.com/ws");
    }

    #[rstest]
    fn test_ws_url_coinm_demo() {
        let url = get_ws_base_url(BinanceProductType::CoinM, BinanceEnvironment::Demo);
        assert_eq!(url, "wss://demo-dstream.binance.com/ws");
    }

    #[rstest]
    fn test_ws_url_options_testnet() {
        let url = get_ws_base_url(BinanceProductType::Options, BinanceEnvironment::Testnet);
        assert_eq!(url, "wss://fstream.binancefuture.com/market/ws");
    }

    #[rstest]
    fn test_ws_url_options_demo() {
        let url = get_ws_base_url(BinanceProductType::Options, BinanceEnvironment::Demo);
        assert_eq!(url, "wss://fstream.binancefuture.com/market/ws");
    }

    #[rstest]
    fn test_ws_private_url_usdm_live() {
        let url = get_ws_private_base_url(BinanceProductType::UsdM, BinanceEnvironment::Live);
        assert_eq!(url, "wss://fstream.binance.com/private/ws");
    }

    #[rstest]
    fn test_ws_private_url_fallback_to_market() {
        let url = get_ws_private_base_url(BinanceProductType::Spot, BinanceEnvironment::Live);
        assert_eq!(
            url,
            get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Live)
        );
    }

    #[rstest]
    fn test_ws_public_url_usdm_live() {
        let url = get_ws_public_base_url(BinanceProductType::UsdM, BinanceEnvironment::Live);
        assert_eq!(url, "wss://fstream.binance.com/public/ws");
    }

    #[rstest]
    fn test_ws_public_url_options_demo() {
        let url = get_ws_public_base_url(BinanceProductType::Options, BinanceEnvironment::Demo);
        assert_eq!(url, "wss://fstream.binancefuture.com/public/ws");
    }

    #[rstest]
    fn test_ws_private_url_options_demo() {
        let url = get_ws_private_base_url(BinanceProductType::Options, BinanceEnvironment::Demo);
        assert_eq!(url, "wss://fstream.binancefuture.com/private/ws");
    }

    #[rstest]
    #[case(
        BinanceProductType::UsdM,
        "wss://fstream.binance.com/private/ws",
        "wss://fstream.binance.com/private/ws?listenKey=redacted"
    )]
    #[case(
        BinanceProductType::CoinM,
        "wss://dstream.binance.com/ws",
        "wss://dstream.binance.com/ws/redacted"
    )]
    #[case(
        BinanceProductType::CoinM,
        "wss://dstream.binancefuture.com/ws",
        "wss://dstream.binancefuture.com/ws/redacted"
    )]
    #[case(
        BinanceProductType::CoinM,
        "wss://demo-dstream.binance.com/ws",
        "wss://demo-dstream.binance.com/ws/redacted"
    )]
    #[case(
        BinanceProductType::CoinM,
        "wss://custom.example/ws/",
        "wss://custom.example/ws/redacted"
    )]
    #[case(
        BinanceProductType::CoinM,
        "wss://custom.example",
        "wss://custom.example/ws/redacted"
    )]
    #[case(
        BinanceProductType::UsdM,
        "ws://127.0.0.1:1234/ws-inject",
        "ws://127.0.0.1:1234/ws-inject?listenKey=redacted"
    )]
    #[case(
        BinanceProductType::CoinM,
        "ws://127.0.0.1:1234/ws-inject",
        "ws://127.0.0.1:1234/ws-inject/redacted"
    )]
    fn test_futures_user_stream_url(
        #[case] product_type: BinanceProductType,
        #[case] base_url: &str,
        #[case] expected: &str,
    ) {
        let url = get_futures_user_stream_url(product_type, base_url, "redacted");
        assert_eq!(url, expected);
    }

    #[rstest]
    #[should_panic(expected = "Futures user stream requires UsdM or CoinM product type")]
    fn test_futures_user_stream_url_rejects_non_futures_product() {
        let _ = get_futures_user_stream_url(
            BinanceProductType::Spot,
            "wss://stream.binance.com/ws",
            "redacted",
        );
    }

    #[rstest]
    fn test_ws_public_url_fallback_to_market() {
        let url = get_ws_public_base_url(BinanceProductType::Spot, BinanceEnvironment::Live);
        assert_eq!(
            url,
            get_ws_base_url(BinanceProductType::Spot, BinanceEnvironment::Live)
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

    #[rstest]
    #[case(BinanceEnvironment::Live, Some("https://api.binance.com"))]
    #[case(BinanceEnvironment::Testnet, None)]
    #[case(BinanceEnvironment::Demo, None)]
    fn test_sapi_base_url(#[case] environment: BinanceEnvironment, #[case] expected: Option<&str>) {
        let url = get_sapi_base_url(environment);
        assert_eq!(url, expected);
    }
}
