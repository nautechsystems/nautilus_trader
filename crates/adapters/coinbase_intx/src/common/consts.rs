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

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const COINBASE_INTX: &str = "COINBASE_INTX";
pub static COINBASE_INTX_VENUE: LazyLock<Venue> =
    LazyLock::new(|| Venue::new(Ustr::from(COINBASE_INTX)));

// Coinbase International Exchange constants
pub const COINBASE_INTX_REST_URL: &str = "https://api.international.coinbase.com";
pub const COINBASE_INTX_REST_SANDBOX_URL: &str = "https://api-n5e1.coinbase.com";
pub const COINBASE_INTX_WS_URL: &str = "wss://ws-md.international.coinbase.com";
pub const COINBASE_INTX_WS_SANDBOX_URL: &str = "wss://ws-md.n5e2.coinbase.com";
pub const COINBASE_INTX_FIX_DROP_COPY: &str = "fix.international.coinbase.com:6130";
