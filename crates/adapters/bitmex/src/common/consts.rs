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

//! BitMEX adapter constants including base URLs and the venue identifier.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const BITMEX: &str = "BITMEX";

pub const BITMEX_WS_URL: &str = "wss://ws.bitmex.com/realtime";
pub const BITMEX_WS_TESTNET_URL: &str = "wss://ws.testnet.bitmex.com/realtime";
pub const BITMEX_HTTP_URL: &str = "https://www.bitmex.com/api/v1";
pub const BITMEX_HTTP_TESTNET_URL: &str = "https://testnet.bitmex.com/api/v1";

pub static BITMEX_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BITMEX)));
