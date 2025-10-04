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

//! Common constants and utilities for the Coinbase adapter.

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// Coinbase venue identifier
pub fn coinbase_venue() -> Venue {
    Venue::new("COINBASE")
}

/// Base URL for Coinbase Advanced Trade API
pub const BASE_URL_PROD: &str = "https://api.coinbase.com";

/// Base URL for WebSocket
pub const WS_URL_PROD: &str = "wss://advanced-trade-ws.coinbase.com";

/// API version
pub const API_VERSION: &str = "v3";

/// User agent for HTTP requests
pub fn get_user_agent() -> String {
    format!("NautilusTrader/{}", env!("CARGO_PKG_VERSION"))
}

/// Parse a Coinbase product ID into base and quote currencies
pub fn parse_product_id(product_id: &str) -> Option<(Ustr, Ustr)> {
    let parts: Vec<&str> = product_id.split('-').collect();
    if parts.len() == 2 {
        Some((Ustr::from(parts[0]), Ustr::from(parts[1])))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_product_id() {
        let (base, quote) = parse_product_id("BTC-USD").unwrap();
        assert_eq!(base.as_str(), "BTC");
        assert_eq!(quote.as_str(), "USD");
    }

    #[test]
    fn test_parse_invalid_product_id() {
        assert!(parse_product_id("INVALID").is_none());
    }
}

