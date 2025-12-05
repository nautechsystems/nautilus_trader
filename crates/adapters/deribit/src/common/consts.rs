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

//! Core constants for the Deribit adapter.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

/// Venue identifier string.
pub const DERIBIT: &str = "DERIBIT";

/// Static venue instance.
pub static DERIBIT_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(DERIBIT)));

// Production URLs
pub const DERIBIT_HTTP_URL: &str = "https://www.deribit.com";
pub const DERIBIT_WS_URL: &str = "wss://www.deribit.com/ws/api/v2";

// Testnet URLs
pub const DERIBIT_TESTNET_HTTP_URL: &str = "https://test.deribit.com";
pub const DERIBIT_TESTNET_WS_URL: &str = "wss://test.deribit.com/ws/api/v2";

// API paths
pub const DERIBIT_API_VERSION: &str = "v2";
pub const DERIBIT_API_PATH: &str = "/api/v2";

// JSON-RPC constants
pub const JSONRPC_VERSION: &str = "2.0";
