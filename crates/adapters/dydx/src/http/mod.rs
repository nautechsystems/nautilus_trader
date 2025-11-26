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
//! This module provides an HTTP client for interacting with dYdX's Indexer REST endpoints,
//! supporting:
//!
//! - **Market data queries**: Perpetual markets, historical trades, OHLCV candles, order books.
//! - **Account information**: Subaccounts, positions, fills, transfers, and funding payments.
//! - **Order queries**: Historical orders and order status.
//! - **Rate limiting**: Automatic rate limiting and retry logic via `nautilus_network::http::HttpClient`.
//!
//! # Architecture
//!
//! The HTTP client follows a two-layer architecture:
//!
//! - **Raw client** ([`client::DydxRawHttpClient`]): Low-level API methods matching Indexer endpoints.
//! - **Domain client** ([`client::DydxHttpClient`]): High-level methods using Nautilus domain types,
//!   wraps raw client in `Arc` for efficient cloning (required for Python bindings).
//!
//! # Authentication
//!
//! The dYdX v4 Indexer REST API is **publicly accessible** and does NOT require
//! authentication or request signing. All endpoints use wallet addresses and subaccount
//! numbers as query parameters.
//!
//! Order submission and trading operations use gRPC with blockchain transaction signing,
//! not the REST API (handled separately in the execution module).
//!
//! # References
//!
//! - dYdX v4 Indexer API: <https://docs.dydx.trade/developers/indexer/indexer_api>
//!
//! # Official documentation
//!
//! See: <https://docs.dydx.exchange/api_integration-indexer/indexer_api>

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;
