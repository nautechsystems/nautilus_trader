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
