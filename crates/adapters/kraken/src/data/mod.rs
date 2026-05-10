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

//! Live market data client implementations for the Kraken adapter.
//!
//! This module provides separate data clients for Kraken Spot and Futures markets:
//!
//! - [`KrakenSpotDataClient`] - For Spot markets using WebSocket v2
//! - [`KrakenFuturesDataClient`] - For Futures markets
//!
//! # Supported Data Types
//!
//! ## Spot
//! - Order book deltas and snapshots
//! - Trade ticks
//! - Quote ticks (best bid/ask)
//! - OHLC bars
//!
//! ## Futures
//! - Order book deltas and snapshots
//! - Trade ticks
//! - Quote ticks (best bid/ask)
//! - Mark prices
//! - Index prices

mod futures;
mod spot;

pub use futures::KrakenFuturesDataClient;
pub use spot::KrakenSpotDataClient;
