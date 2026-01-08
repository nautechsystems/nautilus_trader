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

//! Core constants shared across the AX Exchange adapter components.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const AX: &str = "AX";
pub static AX_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(AX)));

// HTTP endpoints
pub const AX_HTTP_URL: &str = "https://gateway.architect.exchange/api";
pub const AX_HTTP_SANDBOX_URL: &str = "https://gateway.sandbox.architect.exchange/api";

// HTTP order management endpoints (separate base URL)
pub const AX_ORDERS_URL: &str = "https://gateway.architect.exchange/orders";
pub const AX_ORDERS_SANDBOX_URL: &str = "https://gateway.sandbox.architect.exchange/orders";

// Market data WebSocket endpoints
pub const AX_WS_PUBLIC_URL: &str = "wss://gateway.architect.exchange/md/ws";
pub const AX_WS_SANDBOX_PUBLIC_URL: &str = "wss://gateway.sandbox.architect.exchange/md/ws";

// Orders WebSocket endpoints (requires Bearer token authentication)
pub const AX_WS_PRIVATE_URL: &str = "wss://gateway.architect.exchange/orders/ws";
pub const AX_WS_SANDBOX_PRIVATE_URL: &str = "wss://gateway.sandbox.architect.exchange/orders/ws";
