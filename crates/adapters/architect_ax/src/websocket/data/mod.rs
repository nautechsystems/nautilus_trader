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

//! Market data WebSocket client and handler for Ax.

pub mod client;
pub mod handler;
pub mod parse;

pub use client::{AxMdWebSocketClient, AxWsClientError, AxWsResult};
pub use handler::HandlerCommand;
pub use parse::{
    parse_book_l1_quote, parse_book_l2_deltas, parse_book_l3_deltas, parse_trade_tick,
};
