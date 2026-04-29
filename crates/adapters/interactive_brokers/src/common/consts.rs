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

//! Core constants shared across the Interactive Brokers adapter components.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const INTERACTIVE_BROKERS: &str = "INTERACTIVE_BROKERS";
pub const IB: &str = "IB";
pub static IB_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(IB)));

/// Default host for IB Gateway/TWS.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Default port for IB Gateway.
pub const DEFAULT_PORT: u16 = 4002;

/// Default port for TWS.
pub const DEFAULT_TWS_PORT: u16 = 7497;

/// Default client ID.
pub const DEFAULT_CLIENT_ID: i32 = 1;
