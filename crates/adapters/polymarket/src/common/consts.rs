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

//! Constants for the Polymarket adapter.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const POLYMARKET: &str = "POLYMARKET";

pub static POLYMARKET_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(POLYMARKET)));

pub const USDC: &str = "USDC";

pub const MAX_PRICE: &str = "0.999";
pub const MIN_PRICE: &str = "0.001";
pub const MAX_PRECISION_MAKER: u8 = 5;
pub const MAX_PRECISION_TAKER: u8 = 2;

pub const WS_MAX_SUBSCRIPTIONS: usize = 200;
pub const WS_DEFAULT_SUBSCRIPTIONS: usize = 200;

/// Requests per minute.
pub const HTTP_RATE_LIMIT: u32 = 100;

pub const INVALID_API_KEY: &str = "Unauthorized/Invalid api key";
pub const CANCEL_ALREADY_DONE: &str = "already canceled or matched";

/// Polygon chain ID.
pub const CHAIN_ID: u64 = 137;
