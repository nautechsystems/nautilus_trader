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

//! Core constants shared across the Bybit adapter components.

use std::sync::LazyLock;

use nautilus_model::identifiers::{ClientId, Venue};
use ustr::Ustr;

/// Venue identifier string.
pub const BYBIT: &str = "BYBIT";

/// Static venue instance.
pub static BYBIT_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BYBIT)));

/// Static client ID instance.
pub static BYBIT_CLIENT_ID: LazyLock<ClientId> = LazyLock::new(|| ClientId::new(Ustr::from(BYBIT)));

// See <https://www.bybit.com/en/broker> for further details.
pub(crate) const BYBIT_NAUTILUS_BROKER_ID: &str = "Qy000878";

pub(crate) const BYBIT_WS_TOPIC_DELIMITER: char = '.';

pub(crate) const BYBIT_DEFAULT_ORDERBOOK_DEPTH: u32 = 50;
pub(crate) const BYBIT_QUOTE_DEPTH: u32 = 1;

// See <https://bybit-exchange.github.io/docs/v5/websocket/public/orderbook>.
pub(crate) const BYBIT_BOOK_DEPTHS: [u32; 4] = [1, 50, 200, 1000];
