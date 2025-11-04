// Asterdex URL management
use crate::common::consts::*;

#[derive(Clone, Debug)]
pub struct AsterdexUrls {
    base_http_spot: String,
    base_http_futures: String,
    base_ws_spot: String,
    base_ws_futures: String,
}

impl AsterdexUrls {
    pub fn new(
        base_http_spot: Option<String>,
        base_http_futures: Option<String>,
        base_ws_spot: Option<String>,
        base_ws_futures: Option<String>,
    ) -> Self {
        Self {
            base_http_spot: base_http_spot.unwrap_or_else(|| "https://sapi.asterdex.com".to_string()),
            base_http_futures: base_http_futures.unwrap_or_else(|| ASTERDEX_HTTP_BASE_URL.to_string()),
            base_ws_spot: base_ws_spot.unwrap_or_else(|| "wss://sstream.asterdex.com".to_string()),
            base_ws_futures: base_ws_futures.unwrap_or_else(|| ASTERDEX_WS_BASE_URL.to_string()),
        }
    }

    pub fn base_http_spot(&self) -> &str { &self.base_http_spot }
    pub fn base_http_futures(&self) -> &str { &self.base_http_futures }
    pub fn base_ws_spot(&self) -> &str { &self.base_ws_spot }
    pub fn base_ws_futures(&self) -> &str { &self.base_ws_futures }

    // Spot endpoints
    pub fn spot_exchange_info(&self) -> String {
        format!("{}/api/v1/exchangeInfo", self.base_http_spot)
    }

    pub fn spot_order_book(&self, symbol: &str, limit: Option<u32>) -> String {
        let limit_str = limit.map(|l| format!("&limit={}", l)).unwrap_or_default();
        format!("{}/api/v1/depth?symbol={}{}", self.base_http_spot, symbol, limit_str)
    }

    pub fn spot_trades(&self, symbol: &str) -> String {
        format!("{}/api/v1/trades?symbol={}", self.base_http_spot, symbol)
    }

    pub fn spot_account(&self) -> String {
        format!("{}/api/v1/account", self.base_http_spot)
    }

    // Futures endpoints
    pub fn futures_exchange_info(&self) -> String {
        format!("{}/fapi/v1/exchangeInfo", self.base_http_futures)
    }

    pub fn futures_order_book(&self, symbol: &str, limit: Option<u32>) -> String {
        let limit_str = limit.map(|l| format!("&limit={}", l)).unwrap_or_default();
        format!("{}/fapi/v1/depth?symbol={}{}", self.base_http_futures, symbol, limit_str)
    }

    pub fn futures_trades(&self, symbol: &str) -> String {
        format!("{}/fapi/v1/trades?symbol={}", self.base_http_futures, symbol)
    }

    pub fn futures_account(&self) -> String {
        format!("{}/fapi/v4/account", self.base_http_futures)
    }
}
