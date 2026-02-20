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

//! Live execution client implementations for the Kraken adapter.
//!
//! This module provides separate execution clients for Kraken Spot and Futures markets:
//!
//! - [`KrakenSpotExecutionClient`] - For Spot markets using WebSocket v2
//! - [`KrakenFuturesExecutionClient`] - For Futures markets
//!
//! # Supported Operations
//!
//! ## Common
//! - Order submission (market, limit, stop)
//! - Order modification
//! - Order cancellation (single, batch, cancel-all)
//! - Account state and balance queries
//!
//! ## Futures Only
//! - Position management

mod futures;
mod spot;

pub use futures::KrakenFuturesExecutionClient;
pub use spot::KrakenSpotExecutionClient;
