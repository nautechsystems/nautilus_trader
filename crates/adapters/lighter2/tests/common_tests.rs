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

//! Tests for common utilities.

use nautilus_lighter2::common::{
    credential::LighterCredentials,
    enums::{LighterAccountType, LighterOrderSide, LighterOrderType},
    urls::LighterUrls,
    consts::*,
};

#[test]
fn test_credentials_creation() {
    let creds = LighterCredentials::new(
        "test_api_key".to_string(),
        "test_eth_key".to_string(),
        10,
        1,
    );
    assert!(creds.is_ok());

    let creds = creds.unwrap();
    assert_eq!(creds.api_key_index(), 10);
    assert_eq!(creds.account_index(), 1);
}

#[test]
fn test_invalid_api_key_index() {
    // Index 0 is reserved for desktop
    let creds = LighterCredentials::new(
        "test_api_key".to_string(),
        "test_eth_key".to_string(),
        0,
        1,
    );
    assert!(creds.is_err());

    // Index 1 is reserved for mobile
    let creds = LighterCredentials::new(
        "test_api_key".to_string(),
        "test_eth_key".to_string(),
        1,
        1,
    );
    assert!(creds.is_err());

    // Index 255 is for retrieving all keys
    let creds = LighterCredentials::new(
        "test_api_key".to_string(),
        "test_eth_key".to_string(),
        255,
        1,
    );
    assert!(creds.is_err());
}

#[test]
fn test_urls_mainnet() {
    let urls = LighterUrls::new(None, None, false);
    assert_eq!(urls.base_http(), LIGHTER_MAINNET_HTTP_URL);
    assert_eq!(urls.base_ws(), LIGHTER_MAINNET_WS_URL);
}

#[test]
fn test_urls_testnet() {
    let urls = LighterUrls::new(None, None, true);
    assert_eq!(urls.base_http(), LIGHTER_TESTNET_HTTP_URL);
    assert_eq!(urls.base_ws(), LIGHTER_TESTNET_WS_URL);
}

#[test]
fn test_urls_custom() {
    let custom_http = "https://custom.lighter.api";
    let custom_ws = "wss://custom.lighter.ws";
    let urls = LighterUrls::new(
        Some(custom_http.to_string()),
        Some(custom_ws.to_string()),
        false,
    );
    assert_eq!(urls.base_http(), custom_http);
    assert_eq!(urls.base_ws(), custom_ws);
}

#[test]
fn test_url_endpoints() {
    let urls = LighterUrls::new(None, None, false);

    // Test account endpoint
    let account_url = urls.account(Some(1));
    assert!(account_url.contains("account"));
    assert!(account_url.contains("index"));
    assert!(account_url.contains("1"));

    // Test markets endpoint
    let markets_url = urls.markets();
    assert!(markets_url.contains("markets"));

    // Test order book endpoint
    let orderbook_url = urls.order_book(5);
    assert!(orderbook_url.contains("orderbook"));
    assert!(orderbook_url.contains("5"));

    // Test trades endpoint
    let trades_url = urls.trades(10);
    assert!(trades_url.contains("trades"));
    assert!(trades_url.contains("10"));

    // Test nonce endpoint
    let nonce_url = urls.nonce(2);
    assert!(nonce_url.contains("nonce"));
    assert!(nonce_url.contains("2"));
}

#[test]
fn test_account_type_display() {
    let standard = LighterAccountType::Standard;
    assert_eq!(standard.to_string(), "standard");

    let premium = LighterAccountType::Premium;
    assert_eq!(premium.to_string(), "premium");
}

#[test]
fn test_order_side_display() {
    let buy = LighterOrderSide::Buy;
    assert_eq!(buy.to_string(), "buy");

    let sell = LighterOrderSide::Sell;
    assert_eq!(sell.to_string(), "sell");
}

#[test]
fn test_order_type_display() {
    let limit = LighterOrderType::Limit;
    assert_eq!(limit.to_string(), "LIMIT");

    let market = LighterOrderType::Market;
    assert_eq!(market.to_string(), "MARKET");
}

#[test]
fn test_constants() {
    // Verify constants are properly defined
    assert_eq!(LIGHTER, "LIGHTER");
    assert!(!LIGHTER_MAINNET_HTTP_URL.is_empty());
    assert!(!LIGHTER_TESTNET_HTTP_URL.is_empty());
    assert!(!LIGHTER_MAINNET_WS_URL.is_empty());
    assert!(!LIGHTER_TESTNET_WS_URL.is_empty());

    // Verify precision defaults
    assert!(LIGHTER_DEFAULT_PRICE_PRECISION > 0);
    assert!(LIGHTER_DEFAULT_SIZE_PRECISION > 0);
}
