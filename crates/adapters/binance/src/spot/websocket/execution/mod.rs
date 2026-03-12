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

//! Binance Spot User Data Stream (UDS) execution WebSocket support.
//!
//! Provides real-time order and account events via the Binance Spot user data
//! stream. The handler emits raw venue types; the execution client performs all
//! Nautilus conversion.

pub mod client;
pub mod handler;
pub mod messages;
pub mod parse;

pub use client::BinanceSpotUdsClient;
pub use messages::BinanceSpotUdsMessage;
