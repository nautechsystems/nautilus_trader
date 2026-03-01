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

//! URL resolution for the Kalshi API endpoints.

const REST_BASE_URL: &str = "https://api.elections.kalshi.com/trade-api/v2";
const WS_BASE_URL: &str = "wss://api.elections.kalshi.com/trade-api/ws/v2";
const DEMO_REST_BASE_URL: &str = "https://demo-api.kalshi.co/trade-api/v2";
const DEMO_WS_BASE_URL: &str = "wss://demo-api.kalshi.co/trade-api/ws/v2";

#[must_use]
pub const fn rest_base_url() -> &'static str {
    REST_BASE_URL
}

#[must_use]
pub const fn ws_base_url() -> &'static str {
    WS_BASE_URL
}

#[must_use]
pub const fn demo_rest_base_url() -> &'static str {
    DEMO_REST_BASE_URL
}

#[must_use]
pub const fn demo_ws_base_url() -> &'static str {
    DEMO_WS_BASE_URL
}
