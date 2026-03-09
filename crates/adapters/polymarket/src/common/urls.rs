//! URL resolution for the Polymarket API endpoints.

const CLOB_HTTP_URL: &str = "https://clob.polymarket.com";
const CLOB_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws";
const CLOB_WS_MARKET_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const CLOB_WS_USER_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/user";
const GAMMA_API_URL: &str = "https://gamma-api.polymarket.com";

#[must_use]
pub const fn clob_http_url() -> &'static str {
    CLOB_HTTP_URL
}

#[must_use]
pub const fn clob_ws_url() -> &'static str {
    CLOB_WS_URL
}

#[must_use]
pub const fn clob_ws_market_url() -> &'static str {
    CLOB_WS_MARKET_URL
}

#[must_use]
pub const fn clob_ws_user_url() -> &'static str {
    CLOB_WS_USER_URL
}

#[must_use]
pub const fn gamma_api_url() -> &'static str {
    GAMMA_API_URL
}
