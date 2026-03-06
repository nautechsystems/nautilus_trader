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
