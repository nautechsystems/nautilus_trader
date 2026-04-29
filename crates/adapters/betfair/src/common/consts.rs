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

//! Core constants for the Betfair adapter.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// Venue identifier string.
pub const BETFAIR: &str = "BETFAIR";

/// Static venue instance.
pub static BETFAIR_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BETFAIR)));

/// Price decimal precision for Betfair markets.
pub const BETFAIR_PRICE_PRECISION: u8 = 2;

/// Quantity decimal precision for Betfair markets.
pub const BETFAIR_QUANTITY_PRECISION: u8 = 2;

// Identity API (SSO)
pub const BETFAIR_IDENTITY_URL: &str = "https://identitysso-cert.betfair.com/api";

// Betting API (JSON-RPC)
pub const BETFAIR_BETTING_URL: &str = "https://api.betfair.com/exchange/betting/json-rpc/v1";

// Accounts API (JSON-RPC)
pub const BETFAIR_ACCOUNTS_URL: &str = "https://api.betfair.com/exchange/account/json-rpc/v1";

// Navigation API (REST)
pub const BETFAIR_NAVIGATION_URL: &str =
    "https://api.betfair.com/exchange/betting/rest/v1/en/navigation/menu.json";

// Exchange stream (market data and orders)
pub const BETFAIR_STREAM_HOST: &str = "stream-api.betfair.com";

// Race stream (TPD / race tracking data)
pub const BETFAIR_RACE_STREAM_HOST: &str = "sports-data-stream-api.betfair.com";

/// Stream TLS port.
pub const BETFAIR_STREAM_PORT: u16 = 443;

// Interactive login URL (non-cert)
pub const BETFAIR_IDENTITY_LOGIN_URL: &str = "https://identitysso.betfair.com/api/login";

// Keep-alive URL (must match login host: interactive SSO, not cert)
pub const BETFAIR_KEEP_ALIVE_URL: &str = "https://identitysso.betfair.com/api/keepAlive";

// Rate limiter keys
pub const BETFAIR_RATE_LIMIT_DEFAULT: &str = "default";
pub const BETFAIR_RATE_LIMIT_ORDERS: &str = "orders";

/// Maximum length for the `customer_order_ref` field on place instructions.
///
/// Betfair silently truncates longer references. We take the last 32 characters
/// of the client order ID to preserve the high-entropy suffix (UUID tail).
pub const BETFAIR_CUSTOMER_ORDER_REF_MAX_LEN: usize = 32;

// Betting API JSON-RPC methods
pub const METHOD_LIST_MARKET_CATALOGUE: &str = "SportsAPING/v1.0/listMarketCatalogue";
pub const METHOD_LIST_CURRENT_ORDERS: &str = "SportsAPING/v1.0/listCurrentOrders";
pub const METHOD_PLACE_ORDERS: &str = "SportsAPING/v1.0/placeOrders";
pub const METHOD_CANCEL_ORDERS: &str = "SportsAPING/v1.0/cancelOrders";
pub const METHOD_REPLACE_ORDERS: &str = "SportsAPING/v1.0/replaceOrders";

// Accounts API JSON-RPC methods
pub const METHOD_GET_ACCOUNT_FUNDS: &str = "AccountAPING/v1.0/getAccountFunds";
pub const METHOD_GET_ACCOUNT_DETAILS: &str = "AccountAPING/v1.0/getAccountDetails";

// Stream operation strings
pub const STREAM_OP_AUTHENTICATION: &str = "authentication";
pub const STREAM_OP_MARKET_SUBSCRIPTION: &str = "marketSubscription";
pub const STREAM_OP_ORDER_SUBSCRIPTION: &str = "orderSubscription";
pub const STREAM_OP_RACE_SUBSCRIPTION: &str = "raceSubscription";
pub const STREAM_OP_HEARTBEAT: &str = "heartbeat";

// HTTP header names
pub const HEADER_X_AUTHENTICATION: &str = "X-Authentication";
pub const HEADER_X_APPLICATION: &str = "X-Application";

// Default market attribute values
pub const DEFAULT_BETTING_TYPE: &str = "ODDS";
pub const DEFAULT_MARKET_TYPE: &str = "WIN";
