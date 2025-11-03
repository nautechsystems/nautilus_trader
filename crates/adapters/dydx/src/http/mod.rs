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

//! HTTP/REST client implementation for the dYdX v4 Indexer API.
//!
//! This module provides an HTTP client for interacting with dYdX's Indexer REST endpoints, including:
//!
//! - Market data queries (perpetual markets, trades, candles).
//! - Account information (subaccounts, positions, fills).
//! - Order queries and historical data.
//! - Rate limiting and retry logic.
//!
//! # Important Note
//!
//! The dYdX v4 Indexer REST API is **publicly accessible** and does NOT require
//! authentication or request signing. All endpoints use wallet addresses and subaccount
//! numbers as query parameters. Order submission and trading operations use gRPC with
//! blockchain transaction signing, not REST API.
//!
//! # Official documentation
//!
//! See: <https://docs.dydx.exchange/api_integration-indexer/indexer_api>

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;
