//! URL helpers for BitMEX services.

use super::consts::{
    BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL, BITMEX_WS_TESTNET_URL, BITMEX_WS_URL,
};

/// Gets the BitMEX HTTP base URL.
pub fn get_http_base_url(testnet: bool) -> String {
    if testnet {
        BITMEX_HTTP_TESTNET_URL.to_string()
    } else {
        BITMEX_HTTP_URL.to_string()
    }
}

/// Gets the BitMEX WebSocket URL.
pub fn get_ws_url(testnet: bool) -> String {
    if testnet {
        BITMEX_WS_TESTNET_URL.to_string()
    } else {
        BITMEX_WS_URL.to_string()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_http_urls() {
        assert_eq!(get_http_base_url(false), "https://www.bitmex.com/api/v1");
        assert_eq!(get_http_base_url(true), "https://testnet.bitmex.com/api/v1");
    }

    #[rstest]
    fn test_ws_urls() {
        assert_eq!(get_ws_url(false), "wss://ws.bitmex.com/realtime");
        assert_eq!(get_ws_url(true), "wss://ws.testnet.bitmex.com/realtime");
    }
}
