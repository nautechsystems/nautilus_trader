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

//! Core constants shared across the Bybit adapter components.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const BYBIT: &str = "BYBIT";
pub static BYBIT_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BYBIT)));

pub const BYBIT_PONG: &str = "pong";

/// See <https://www.bybit.com/en/broker> for further details.
pub const BYBIT_NAUTILUS_BROKER_ID: &str = "Qy000878";

pub const BYBIT_HTTP_URL: &str = "https://api.bybit.com";
pub const BYBIT_HTTP_TESTNET_URL: &str = "https://api-testnet.bybit.com";

pub const BYBIT_WS_PUBLIC_URL: &str = "wss://stream.bybit.com/v5/public/linear";
pub const BYBIT_WS_PRIVATE_URL: &str = "wss://stream.bybit.com/v5/private";

pub const BYBIT_WS_TESTNET_PUBLIC_URL: &str = "wss://stream-testnet.bybit.com/v5/public/linear";
pub const BYBIT_WS_TESTNET_PRIVATE_URL: &str = "wss://stream-testnet.bybit.com/v5/private";
