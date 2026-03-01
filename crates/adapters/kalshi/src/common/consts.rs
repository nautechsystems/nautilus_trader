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

//! Constants for the Kalshi adapter.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const KALSHI: &str = "KALSHI";

pub static KALSHI_VENUE: LazyLock<Venue> =
    LazyLock::new(|| Venue::new(Ustr::from(KALSHI)));

pub const USD: &str = "USD";

/// Minimum YES price (dollar string).
pub const MIN_PRICE: &str = "0.0001";
/// Maximum YES price (dollar string).
pub const MAX_PRICE: &str = "0.9999";

/// Price precision: 4 decimal places (supports subpenny pricing).
pub const PRICE_PRECISION: u8 = 4;
/// Size precision: 2 decimal places.
pub const SIZE_PRECISION: u8 = 2;

/// Default REST requests per second (Basic tier).
pub const HTTP_RATE_LIMIT_RPS: u32 = 20;
