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

//! HTTP client bindings for the dYdX adapter.
//!
//! This module provides an HTTP client for interacting with the dYdX v4 Indexer REST API.
//! It handles:
//! - Request signing and authentication.
//! - Rate limiting and retry logic.
//! - Request/response models.
//! - Parsing dYdX data into Nautilus domain models.
//!
//! The client supports dYdX REST endpoints including:
//! - Market data (instruments, trades, candles).
//! - Account data (subaccounts, positions, fills).
//! - Order management queries.
//! - Historical data.
